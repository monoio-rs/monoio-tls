use std::fmt;

use monoio::io::{AsyncReadRent, AsyncWriteRent};

use crate::{
    utils::{handshake, IOWrapper},
    TlsError, TlsStream,
};

/// A wrapper around a `native_tls::TlsConnector`, providing an async `connect`
/// method.
#[derive(Clone)]
pub struct TlsConnector {
    inner: native_tls::TlsConnector,
    read_buffer: Option<usize>,
    write_buffer: Option<usize>,
}

impl TlsConnector {
    /// Connects the provided stream with this connector, assuming the provided
    /// domain.
    ///
    /// This function will internally call `TlsConnector::connect` to connect
    /// the stream and returns a future representing the resolution of the
    /// connection operation. The returned future will resolve to either
    /// `TlsStream<S>` or `Error` depending if it's successful or not.
    ///
    /// This is typically used for clients who have already established, for
    /// example, a TCP connection to a remote server. That stream is then
    /// provided here to perform the client half of a connection to a
    /// TLS-powered server.
    pub async fn connect<S>(&self, domain: &str, stream: S) -> Result<TlsStream<S>, TlsError>
    where
        S: AsyncReadRent + AsyncWriteRent,
    {
        let io = IOWrapper::new_with_buffer_size(stream, self.read_buffer, self.write_buffer);
        handshake(move |s_wrap| self.inner.connect(domain, s_wrap), io).await
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

impl fmt::Debug for TlsConnector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TlsConnector").finish()
    }
}

impl From<native_tls::TlsConnector> for TlsConnector {
    fn from(inner: native_tls::TlsConnector) -> TlsConnector {
        TlsConnector {
            inner,
            read_buffer: None,
            write_buffer: None,
        }
    }
}
