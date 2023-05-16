use std::{fmt::Debug, io, mem};

use monoio::{
    buf::{IoBuf, IoBufMut},
    io::{AsyncReadRent, AsyncWriteRent, AsyncWriteRentExt},
};

const BUFFER_SIZE: usize = 16 * 1024;

struct Buffer {
    read: usize,
    write: usize,
    buf: Box<[u8]>,
}

impl Default for Buffer {
    fn default() -> Self {
        Self::new(BUFFER_SIZE)
    }
}

impl Buffer {
    fn new(size: usize) -> Self {
        Self {
            read: 0,
            write: 0,
            buf: vec![0; size].into_boxed_slice(),
        }
    }

    fn len(&self) -> usize {
        self.write - self.read
    }

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn available(&self) -> usize {
        self.buf.len() - self.write
    }

    fn is_full(&self) -> bool {
        self.available() == 0
    }

    fn advance(&mut self, n: usize) {
        assert!(self.read + n <= self.write);
        self.read += n;
        if self.read == self.write {
            self.read = 0;
            self.write = 0;
        }
    }
}

unsafe impl monoio::buf::IoBuf for Buffer {
    fn read_ptr(&self) -> *const u8 {
        unsafe { self.buf.as_ptr().add(self.read) }
    }

    fn bytes_init(&self) -> usize {
        self.write - self.read
    }
}

unsafe impl monoio::buf::IoBufMut for Buffer {
    fn write_ptr(&mut self) -> *mut u8 {
        unsafe { self.buf.as_mut_ptr().add(self.write) }
    }

    fn bytes_total(&mut self) -> usize {
        self.buf.len() - self.write
    }

    unsafe fn set_init(&mut self, pos: usize) {
        self.write += pos;
    }
}

pub struct SafeRead {
    // the option is only meant for temporary take, it always should be some
    buffer: Option<Buffer>,
    status: ReadStatus,
}

impl Debug for SafeRead {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SafeRead")
            .field("status", &self.status)
            .finish()
    }
}

#[derive(Debug)]
enum ReadStatus {
    Eof,
    Err(io::Error),
    Ok,
}

impl Default for SafeRead {
    fn default() -> Self {
        Self {
            buffer: Some(Buffer::default()),
            status: ReadStatus::Ok,
        }
    }
}

impl SafeRead {
    /// Create a new SafeRead with given buffer size.
    pub fn new(buffer_size: usize) -> Self {
        Self {
            buffer: Some(Buffer::new(buffer_size)),
            status: ReadStatus::Ok,
        }
    }

    /// `do_io` do async read from io to inner buffer.
    /// # Handle return value
    /// _: the read result.
    pub async fn do_io<IO: AsyncReadRent>(&mut self, mut io: IO) -> io::Result<usize> {
        // if there are some data inside the buffer, just return.
        let buffer = self.buffer.as_ref().expect("buffer ref expected");
        if !buffer.is_empty() {
            return Ok(buffer.len());
        }

        // read from raw io
        // # Safety
        // We have already checked it is not None.
        let buffer = unsafe { self.buffer.take().unwrap_unchecked() };
        let (result, buf) = io.read(buffer).await;
        self.buffer = Some(buf);
        match result {
            Ok(0) => {
                self.status = ReadStatus::Eof;
                result
            }
            Ok(_) => {
                self.status = ReadStatus::Ok;
                result
            }
            Err(e) => {
                let rerr = e.kind().into();
                self.status = ReadStatus::Err(e);
                Err(rerr)
            }
        }
    }
}

impl io::Read for SafeRead {
    /// `read` from buffer.
    /// # Handle return value
    /// 1. Err(WouldBlock): the buffer is empty, call do_io to fetch more.
    /// 2. _: handle it.
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // if buffer is empty, return WoundBlock.
        let buffer = self.buffer.as_mut().expect("buffer mut expected");
        if buffer.is_empty() {
            return match mem::replace(&mut self.status, ReadStatus::Ok) {
                ReadStatus::Eof => Ok(0),
                ReadStatus::Err(e) => Err(e),
                ReadStatus::Ok => Err(io::ErrorKind::WouldBlock.into()),
            };
        }

        // now buffer is not empty. copy it.
        let to_copy = buffer.len().min(buf.len());
        unsafe { std::ptr::copy_nonoverlapping(buffer.read_ptr(), buf.as_mut_ptr(), to_copy) };
        buffer.advance(to_copy);

        Ok(to_copy)
    }
}

pub struct SafeWrite {
    // the option is only meant for temporary take, it always should be some
    buffer: Option<Buffer>,
    status: WriteStatus,
}

impl Debug for SafeWrite {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SafeWrite")
            .field("status", &self.status)
            .finish()
    }
}

#[derive(Debug)]
enum WriteStatus {
    Err(io::Error),
    Ok,
}

impl Default for SafeWrite {
    fn default() -> Self {
        Self {
            buffer: Some(Buffer::default()),
            status: WriteStatus::Ok,
        }
    }
}

impl SafeWrite {
    /// Create a new SafeWrite with given buffer size.
    pub fn new(buffer_size: usize) -> Self {
        Self {
            buffer: Some(Buffer::new(buffer_size)),
            status: WriteStatus::Ok,
        }
    }

    /// `do_io` do async write from inner buffer to io.
    /// # Handle return value
    /// _: the write_all result(note: the data may have been written even when error).
    pub async fn do_io<IO: AsyncWriteRent>(&mut self, mut io: IO) -> io::Result<usize> {
        // if the buffer is empty, just return.
        let buffer = self.buffer.as_ref().expect("buffer ref expected");
        if buffer.is_empty() {
            return Ok(0);
        }

        // buffer is not empty now. write it.
        // # Safety
        // We have already checked it is not None.
        let buffer = unsafe { self.buffer.take().unwrap_unchecked() };
        let (result, buffer) = io.write_all(buffer).await;
        self.buffer = Some(buffer);
        match result {
            Ok(written_len) => {
                unsafe { self.buffer.as_mut().unwrap_unchecked().advance(written_len) };
                Ok(written_len)
            }
            Err(e) => {
                let rerr = e.kind().into();
                self.status = WriteStatus::Err(e);
                Err(rerr)
            }
        }
    }
}

impl io::Write for SafeWrite {
    /// `write` to buffer.
    /// # Handle return value
    /// 1. Err(WouldBlock): the buffer is full, call do_io to flush it.
    /// 2. _: handle it.
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // if there is too much data inside the buffer, return WoundBlock
        let buffer = self.buffer.as_mut().expect("buffer mut expected");
        match mem::replace(&mut self.status, WriteStatus::Ok) {
            WriteStatus::Err(e) => return Err(e),
            WriteStatus::Ok if buffer.is_full() => return Err(io::ErrorKind::WouldBlock.into()),
            _ => (),
        }

        // there is space inside the buffer, copy to it.
        let to_copy = buf.len().min(buffer.available());
        unsafe { std::ptr::copy_nonoverlapping(buf.as_ptr(), buffer.write_ptr(), to_copy) };
        unsafe { buffer.set_init(to_copy) };
        Ok(to_copy)
    }

    /// `flush` to buffer.
    /// # Handle return value
    /// 1. Err(WouldBlock): the buffer is full, call do_io to flush it.
    /// 2. _: handle it.
    fn flush(&mut self) -> io::Result<()> {
        let buffer = self.buffer.as_mut().expect("buffer mut expected");
        match mem::replace(&mut self.status, WriteStatus::Ok) {
            WriteStatus::Err(e) => Err(e),
            WriteStatus::Ok if !buffer.is_empty() => Err(io::ErrorKind::WouldBlock.into()),
            _ => Ok(()),
        }
    }
}
