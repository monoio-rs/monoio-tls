use std::sync::Arc;

use monoio::io::{AsyncReadRent, AsyncWriteRent};
use rustls::{Connection, ServerConfig, ServerConnection};

use crate::{stream::Stream, TlsError};

/// A wrapper around an underlying raw stream which implements the TLS protocol.
pub type TlsStream<IO> = Stream<IO>;

/// A wrapper around a `rustls::ServerConfig`, providing an async `accept` method.
#[derive(Clone)]
pub struct TlsAcceptor {
    inner: Arc<ServerConfig>,
}

impl From<Arc<ServerConfig>> for TlsAcceptor {
    fn from(inner: Arc<ServerConfig>) -> TlsAcceptor {
        TlsAcceptor { inner }
    }
}

impl From<ServerConfig> for TlsAcceptor {
    fn from(inner: ServerConfig) -> TlsAcceptor {
        TlsAcceptor {
            inner: Arc::new(inner),
        }
    }
}

impl TlsAcceptor {
    pub async fn accept<IO>(&self, stream: IO) -> Result<TlsStream<IO>, TlsError>
    where
        IO: AsyncReadRent + AsyncWriteRent,
    {
        let session = ServerConnection::new(self.inner.clone())?;
        let mut stream = Stream::new(stream, Connection::Server(session));
        stream.handshake().await?;
        Ok(stream)
    }
}
