use std::{cell::UnsafeCell, io, rc::Rc};

use monoio::io::{AsyncReadRent, AsyncWriteRent};
use monoio_io_wrapper::{ReadBuffer, WriteBuffer};
use native_tls::HandshakeError as NativeHandshakeError;

use crate::{TlsError, TlsStream};

#[derive(Debug, Clone)]
pub(crate) struct Buffers {
    r_buffer: Rc<UnsafeCell<ReadBuffer>>,
    w_buffer: Rc<UnsafeCell<WriteBuffer>>,
}

#[derive(Debug)]
pub(crate) struct IOWrapper<IO> {
    io: IO,
    r_buffer: Rc<UnsafeCell<ReadBuffer>>,
    w_buffer: Rc<UnsafeCell<WriteBuffer>>,
}

impl<IO> IOWrapper<IO> {
    pub(crate) fn new(io: IO, r_buffer: ReadBuffer, w_buffer: WriteBuffer) -> Self {
        Self {
            io,
            r_buffer: Rc::new(UnsafeCell::new(r_buffer)),
            w_buffer: Rc::new(UnsafeCell::new(w_buffer)),
        }
    }

    pub(crate) fn new_with_buffer_size(io: IO, r: Option<usize>, w: Option<usize>) -> Self {
        let r_buffer = match r {
            Some(rb) => ReadBuffer::new(rb),
            None => ReadBuffer::default(),
        };
        let w_buffer = match w {
            Some(rb) => WriteBuffer::new(rb),
            None => WriteBuffer::default(),
        };

        Self::new(io, r_buffer, w_buffer)
    }

    pub(crate) fn buffers(&self) -> Buffers {
        Buffers {
            r_buffer: self.r_buffer.clone(),
            w_buffer: self.w_buffer.clone(),
        }
    }

    pub(crate) fn into_parts(
        self,
    ) -> (IO, Rc<UnsafeCell<ReadBuffer>>, Rc<UnsafeCell<WriteBuffer>>) {
        (self.io, self.r_buffer, self.w_buffer)
    }

    pub(crate) async unsafe fn do_read_io(&mut self) -> std::io::Result<usize>
    where
        IO: AsyncReadRent,
    {
        (*self.r_buffer.get()).do_io(&mut self.io).await
    }

    pub(crate) async unsafe fn do_write_io(&mut self) -> std::io::Result<usize>
    where
        IO: AsyncWriteRent,
    {
        (*self.w_buffer.get()).do_io(&mut self.io).await
    }
}

impl<IO: AsyncReadRent> IOWrapper<IO> {
    #[inline]
    #[allow(clippy::await_holding_refcell_ref)]
    pub(crate) async fn read_io(&mut self) -> io::Result<usize> {
        unsafe { &mut *self.r_buffer.get() }
            .do_io(&mut self.io)
            .await
    }
}

impl<IO: AsyncWriteRent> IOWrapper<IO> {
    #[inline]
    #[allow(clippy::await_holding_refcell_ref)]
    pub(crate) async fn write_io(&mut self) -> io::Result<usize> {
        unsafe { &mut *self.w_buffer.get() }
            .do_io(&mut self.io)
            .await
    }
}

impl io::Read for Buffers {
    #[inline]
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        unsafe { &mut *self.r_buffer.get() }.read(buf)
    }
}

impl io::Write for Buffers {
    #[inline]
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        unsafe { &mut *self.w_buffer.get() }.write(buf)
    }

    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        // Due to openssl and rust-openssl issue, in
        // flush we cannot return WouldBlock.
        // Related PRs:
        // https://github.com/openssl/openssl/pull/20919
        // https://github.com/sfackler/rust-openssl/pull/1922
        // After these PRs are merged, we should use:
        // unsafe { &mut *self.w_buffer.get() }.flush()
        Ok(())
    }
}

pub(crate) async fn handshake<F, S>(f: F, mut io: IOWrapper<S>) -> Result<TlsStream<S>, TlsError>
where
    F: FnOnce(Buffers) -> Result<native_tls::TlsStream<Buffers>, NativeHandshakeError<Buffers>>,
    S: AsyncReadRent + AsyncWriteRent,
{
    let mut mid = match f(io.buffers()) {
        Ok(tls) => {
            io.write_io().await?;
            return Ok(TlsStream::new(tls, io));
        }
        Err(NativeHandshakeError::WouldBlock(s)) => s,
        Err(NativeHandshakeError::Failure(e)) => return Err(e.into()),
    };

    loop {
        if io.write_io().await? == 0 {
            io.read_io().await?;
        }

        match mid.handshake() {
            Ok(tls) => {
                io.write_io().await?;
                return Ok(TlsStream::new(tls, io));
            }
            Err(NativeHandshakeError::WouldBlock(s)) => mid = s,
            Err(NativeHandshakeError::Failure(e)) => return Err(e.into()),
        }
    }
}
