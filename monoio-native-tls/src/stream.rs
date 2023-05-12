use std::{
    future::Future,
    io::{self, Read, Write},
};

use monoio::{
    buf::{IoBuf, IoBufMut, IoVecBuf, IoVecBufMut, RawBuf},
    io::{AsyncReadRent, AsyncWriteRent, Split},
    BufResult,
};

use crate::utils::{Buffers, IOWrapper};

/// A wrapper around an underlying raw stream which implements the TLS or SSL
/// protocol.
///
/// A `TlsStream<S>` represents a handshake that has been completed successfully
/// and both the server and the client are ready for receiving and sending
/// data. Bytes read from a `TlsStream` are decrypted from `S` and bytes written
/// to a `TlsStream` are encrypted when passing through to `S`.
#[derive(Debug)]
pub struct TlsStream<S> {
    tls: native_tls::TlsStream<Buffers>,
    io: IOWrapper<S>,
}

impl<S> TlsStream<S> {
    pub(crate) fn new(tls_stream: native_tls::TlsStream<Buffers>, io: IOWrapper<S>) -> Self {
        Self {
            tls: tls_stream,
            io,
        }
    }

    pub fn into_inner(self) -> S {
        self.io.into_parts().0
    }
}

unsafe impl<S: Split> Split for TlsStream<S> {}

impl<S: AsyncReadRent> AsyncReadRent for TlsStream<S> {
    type ReadFuture<'a, T> = impl Future<Output = BufResult<usize, T>> + 'a
    where
        T: IoBufMut + 'a, Self: 'a;

    type ReadvFuture<'a, T> = impl Future<Output = BufResult<usize, T>> + 'a
    where
        T: IoVecBufMut + 'a, Self: 'a;

    #[allow(clippy::await_holding_refcell_ref)]
    fn read<T: IoBufMut>(&mut self, mut buf: T) -> Self::ReadFuture<'_, T> {
        async move {
            let slice =
                unsafe { std::slice::from_raw_parts_mut(buf.write_ptr(), buf.bytes_total()) };

            loop {
                // read from native-tls to buffer
                match self.tls.read(slice) {
                    Ok(n) => {
                        unsafe { buf.set_init(n) };
                        return (Ok(n), buf);
                    }
                    // we need more data, read something.
                    Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => (),
                    Err(e) => {
                        return (Err(e), buf);
                    }
                }

                // now we need data, read something into native-tls
                match unsafe { self.io.do_read_io() }.await {
                    Ok(0) => {
                        return (Ok(0), buf);
                    }
                    Ok(_) => (),
                    Err(e) => {
                        return (Err(e), buf);
                    }
                };
            }
        }
    }

    fn readv<T: IoVecBufMut>(&mut self, mut buf: T) -> Self::ReadvFuture<'_, T> {
        async move {
            let n = match unsafe { RawBuf::new_from_iovec_mut(&mut buf) } {
                Some(raw_buf) => self.read(raw_buf).await.0,
                None => Ok(0),
            };
            if let Ok(n) = n {
                unsafe { buf.set_init(n) };
            }
            (n, buf)
        }
    }
}

impl<S: AsyncWriteRent> AsyncWriteRent for TlsStream<S> {
    type WriteFuture<'a, T> = impl Future<Output = BufResult<usize, T>> + 'a
    where
        T: IoBuf + 'a, Self: 'a;

    type WritevFuture<'a, T> = impl Future<Output = BufResult<usize, T>> + 'a
    where
        T: IoVecBuf + 'a, Self: 'a;

    type FlushFuture<'a> = impl Future<Output = io::Result<()>> + 'a
    where
        Self: 'a;

    type ShutdownFuture<'a> = impl Future<Output = io::Result<()>> + 'a
    where
        Self: 'a;

    #[allow(clippy::await_holding_refcell_ref)]
    fn write<T: IoBuf>(&mut self, buf: T) -> Self::WriteFuture<'_, T> {
        async move {
            // construct slice
            let slice = unsafe { std::slice::from_raw_parts(buf.read_ptr(), buf.bytes_init()) };

            loop {
                // write slice to native-tls and buffer
                let maybe_n = match self.tls.write(slice) {
                    Ok(n) => Some(n),
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => None,
                    Err(e) => return (Err(e), buf),
                };

                // write from buffer to connection
                if let Err(e) = unsafe { self.io.do_write_io() }.await {
                    return (Err(e), buf);
                }

                if let Some(n) = maybe_n {
                    return (Ok(n), buf);
                }
            }
        }
    }

    // TODO: use real writev
    fn writev<T: IoVecBuf>(&mut self, buf_vec: T) -> Self::WritevFuture<'_, T> {
        async move {
            let n = match unsafe { RawBuf::new_from_iovec(&buf_vec) } {
                Some(raw_buf) => self.write(raw_buf).await.0,
                None => Ok(0),
            };
            (n, buf_vec)
        }
    }

    #[allow(clippy::await_holding_refcell_ref)]
    fn flush(&mut self) -> Self::FlushFuture<'_> {
        async move {
            loop {
                match self.tls.flush() {
                    Ok(_) => {
                        unsafe { self.io.do_write_io() }.await?;
                        return Ok(());
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        unsafe { self.io.do_write_io() }.await?;
                    }
                    Err(e) => {
                        return Err(e);
                    }
                }
            }
        }
    }

    fn shutdown(&mut self) -> Self::ShutdownFuture<'_> {
        async move {
            self.tls.shutdown()?;
            unsafe { self.io.do_write_io() }.await?;
            Ok(())
        }
    }
}
