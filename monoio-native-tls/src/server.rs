use std::fmt;

use monoio::io::{AsyncReadRent, AsyncWriteRent};

use crate::{
    utils::{handshake, IOWrapper},
    TlsError, TlsStream,
};

/// A wrapper around a `native_tls::TlsAcceptor`, providing an async `accept`
/// method.
#[derive(Clone)]
pub struct TlsAcceptor {
    inner: native_tls::TlsAcceptor,
    read_buffer: Option<usize>,
    write_buffer: Option<usize>,
}

impl TlsAcceptor {
    /// Accepts a new client connection with the provided stream.
    ///
    /// This function will internally call `TlsAcceptor::accept` to connect
    /// the stream and returns a future representing the resolution of the
    /// connection operation. The returned future will resolve to either
    /// `TlsStream<S>` or `Error` depending if it's successful or not.
    ///
    /// This is typically used after a new socket has been accepted from a
    /// `TcpListener`. That socket is then passed to this function to perform
    /// the server half of accepting a client connection.
    pub async fn accept<S>(&self, stream: S) -> Result<TlsStream<S>, TlsError>
    where
        S: AsyncReadRent + AsyncWriteRent,
    {
        let io = IOWrapper::new_with_buffer_size(stream, self.read_buffer, self.write_buffer);
        handshake(move |s_wrap| self.inner.accept(s_wrap), io).await
    }

    pub fn read_buffer(mut self, size: Option<usize>) -> Self {
        self.read_buffer = size;
        self
    }

    pub fn write_buffer(mut self, size: Option<usize>) -> Self {
        self.write_buffer = size;
        self
    }
}

impl fmt::Debug for TlsAcceptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TlsAcceptor").finish()
    }
}

impl From<native_tls::TlsAcceptor> for TlsAcceptor {
    fn from(inner: native_tls::TlsAcceptor) -> TlsAcceptor {
        TlsAcceptor {
            inner,
            read_buffer: None,
            write_buffer: None,
        }
    }
}
