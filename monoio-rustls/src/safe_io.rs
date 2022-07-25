use std::io;

use bytes::{Buf, BufMut, BytesMut};
use monoio::io::{AsyncReadRent, AsyncWriteRent};

const BUFFER_SIZE: usize = 16 * 1024;

pub(crate) struct SafeRead {
    // the option is only meant for temporary take, it always should be some
    buffer: Option<BytesMut>,
}

impl Default for SafeRead {
    fn default() -> Self {
        Self {
            buffer: Some(BytesMut::default()),
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
        let mut buffer = self.buffer.take().expect("buffer ownership expected");
        buffer.reserve(BUFFER_SIZE);
        let (result, buf) = io.read(buffer).await;
        self.buffer = Some(buf);
        result
    }
}

impl io::Read for SafeRead {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // if buffer is empty, return WoundBlock.
        let buffer = self.buffer.as_mut().expect("buffer mut expected");
        if buffer.is_empty() {
            return Err(io::ErrorKind::WouldBlock.into());
        }

        // now buffer is not empty. copy it.
        let to_copy = buffer.len().min(buf.len());
        unsafe { std::ptr::copy_nonoverlapping(buffer.as_ptr(), buf.as_mut_ptr(), to_copy) };
        buffer.advance(to_copy);

        Ok(to_copy)
    }
}

pub(crate) struct SafeWrite {
    // the option is only meant for temporary take, it always should be some
    buffer: Option<BytesMut>,
}

impl Default for SafeWrite {
    fn default() -> Self {
        Self {
            buffer: Some(BytesMut::default()),
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
        let (result, buffer) = io.write(buffer).await;
        self.buffer = Some(buffer);
        let written_len = result?;
        unsafe { self.buffer.as_mut().unwrap_unchecked().advance(written_len) };

        Ok(written_len)
    }
}

impl io::Write for SafeWrite {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // if there is too much data inside the buffer, return WoundBlock
        let buffer = self.buffer.as_mut().expect("buffer mut expected");
        if buffer.len() >= BUFFER_SIZE {
            return Err(io::ErrorKind::WouldBlock.into());
        }

        // there is space inside the buffer, copy it.
        let space_left = BUFFER_SIZE - buffer.len();
        buffer.reserve(space_left);
        let to_copy = buf.len().min(space_left);
        unsafe { std::ptr::copy_nonoverlapping(buf.as_ptr(), buffer.as_mut_ptr(), to_copy) };
        unsafe { buffer.advance_mut(to_copy) };
        Ok(to_copy)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
