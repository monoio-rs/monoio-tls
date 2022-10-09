use std::{fmt::Debug, hint::unreachable_unchecked, io};

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

impl Buffer {
    fn new() -> Self {
        Self {
            read: 0,
            write: 0,
            buf: vec![0; BUFFER_SIZE].into_boxed_slice(),
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
        assert!(self.write - self.read >= n);
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

pub(crate) struct SafeRead {
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
            buffer: Some(Buffer::new()),
            status: ReadStatus::Ok,
        }
    }
}

impl SafeRead {
    pub(crate) async fn do_io<IO: AsyncReadRent>(&mut self, mut io: IO) -> io::Result<usize> {
        // if there are some data inside the buffer, just return.
        let buffer = self.buffer.as_ref().expect("buffer ref expected");
        if !buffer.is_empty() {
            return Ok(buffer.len());
        }

        // read from raw io
        let buffer = self.buffer.take().expect("buffer ownership expected");
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
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.buffer.is_none() {
            return Err(io::ErrorKind::Other.into());
        }
        // if buffer is empty, return WoundBlock.
        let buffer = self.buffer.as_mut().expect("buffer mut expected");
        if buffer.is_empty() {
            if !matches!(self.status, ReadStatus::Ok) {
                match std::mem::replace(&mut self.status, ReadStatus::Ok) {
                    ReadStatus::Eof => return Ok(0),
                    ReadStatus::Err(e) => return Err(e),
                    ReadStatus::Ok => unsafe { unreachable_unchecked() },
                }
            }
            return Err(io::ErrorKind::WouldBlock.into());
        }

        // now buffer is not empty. copy it.
        let to_copy = buffer.len().min(buf.len());
        unsafe { std::ptr::copy_nonoverlapping(buffer.read_ptr(), buf.as_mut_ptr(), to_copy) };
        buffer.advance(to_copy);

        Ok(to_copy)
    }
}

pub(crate) struct SafeWrite {
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
            buffer: Some(Buffer::new()),
            status: WriteStatus::Ok,
        }
    }
}

impl SafeWrite {
    pub(crate) async fn do_io<IO: AsyncWriteRent>(&mut self, mut io: IO) -> io::Result<usize> {
        // if the buffer is empty, just return.
        let buffer = self.buffer.as_ref().expect("buffer ref expected");
        if buffer.is_empty() {
            return Ok(0);
        }

        // buffer is not empty now. write it.
        let buffer = self.buffer.take().expect("buffer ownership expected");
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
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.buffer.is_none() {
            return Err(io::ErrorKind::Other.into());
        }
        // if there is too much data inside the buffer, return WoundBlock
        let buffer = self.buffer.as_mut().expect("buffer mut expected");
        if !matches!(self.status, WriteStatus::Ok) {
            match std::mem::replace(&mut self.status, WriteStatus::Ok) {
                WriteStatus::Err(e) => return Err(e),
                WriteStatus::Ok => unsafe { unreachable_unchecked() },
            }
        }
        if buffer.is_full() {
            return Err(io::ErrorKind::WouldBlock.into());
        }

        // there is space inside the buffer, copy to it.
        let to_copy = buf.len().min(buffer.available());
        unsafe { std::ptr::copy_nonoverlapping(buf.as_ptr(), buffer.write_ptr(), to_copy) };
        unsafe { buffer.set_init(to_copy) };
        Ok(to_copy)
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.buffer.is_none() {
            return Err(io::ErrorKind::Other.into());
        }
        let buffer = self.buffer.as_mut().expect("buffer mut expected");
        if !matches!(self.status, WriteStatus::Ok) {
            match std::mem::replace(&mut self.status, WriteStatus::Ok) {
                WriteStatus::Err(e) => return Err(e),
                WriteStatus::Ok => unsafe { unreachable_unchecked() },
            }
        }
        if !buffer.is_empty() {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        Ok(())
    }
}
