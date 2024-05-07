use std::io::{self, Read, Write};

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

    #[cfg(feature = "alpn")]
    pub fn alpn_protocol(&self) -> Option<Vec<u8>> {
        self.tls.negotiated_alpn().ok().flatten()
    }
}

unsafe impl<S: Split> Split for TlsStream<S> {}

impl<S: AsyncReadRent> AsyncReadRent for TlsStream<S> {
    #[allow(clippy::await_holding_refcell_ref)]
    async fn read<T: IoBufMut>(&mut self, mut buf: T) -> BufResult<usize, T> {
        let slice = unsafe { std::slice::from_raw_parts_mut(buf.write_ptr(), buf.bytes_total()) };

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

    async fn readv<T: IoVecBufMut>(&mut self, mut buf: T) -> BufResult<usize, T> {
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

impl<S: AsyncWriteRent> AsyncWriteRent for TlsStream<S> {
    #[allow(clippy::await_holding_refcell_ref)]
    async fn write<T: IoBuf>(&mut self, buf: T) -> BufResult<usize, T> {
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

    // TODO: use real writev
    async fn writev<T: IoVecBuf>(&mut self, buf_vec: T) -> BufResult<usize, T> {
        let n = match unsafe { RawBuf::new_from_iovec(&buf_vec) } {
            Some(raw_buf) => self.write(raw_buf).await.0,
            None => Ok(0),
        };
        (n, buf_vec)
    }

    #[allow(clippy::await_holding_refcell_ref)]
    async fn flush(&mut self) -> io::Result<()> {
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

    async fn shutdown(&mut self) -> io::Result<()> {
        self.tls.shutdown()?;
        unsafe { self.io.do_write_io() }.await?;
        Ok(())
    }
}
