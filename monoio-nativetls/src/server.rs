use std::sync::Arc;

use anyhow::bail;
use monoio::io::{AsyncReadRent, AsyncWriteRent};
use native_tls::HandshakeError;

use crate::{handshake::HandshakeStream, std_adapter::StdAdapter, stream::Stream};

/// A wrapper around a `rustls::ServerConfig`, providing an async `accept` method.
#[derive(Clone)]
pub struct TlsAcceptor {
    inner: Arc<native_tls::TlsAcceptor>,
}

impl From<Arc<native_tls::TlsAcceptor>> for TlsAcceptor {
    fn from(inner: Arc<native_tls::TlsAcceptor>) -> TlsAcceptor {
        TlsAcceptor { inner }
    }
}

impl From<native_tls::TlsAcceptor> for TlsAcceptor {
    fn from(inner: native_tls::TlsAcceptor) -> TlsAcceptor {
        TlsAcceptor {
            inner: Arc::new(inner),
        }
    }
}

impl TlsAcceptor {
    pub async fn accept<IO>(&self, stream: IO) -> anyhow::Result<Stream<IO>>
    where
        IO: AsyncReadRent + AsyncWriteRent + 'static,
    {
        match self.inner.clone().accept(StdAdapter::new(stream)) {
            Ok(stream) => Ok(Stream::new(stream)),
            Err(HandshakeError::WouldBlock(stream)) => {
                HandshakeStream::new(stream).handshake().await
            }
            Err(_) => bail!("tls handshake error"),
        }
    }
}
