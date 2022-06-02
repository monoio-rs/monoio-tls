use std::sync::Arc;

use monoio::io::{AsyncReadRent, AsyncWriteRent};
use rustls::{ServerConfig, ServerConnection};

use crate::{
    split::{ReadHalf, WriteHalf},
    stream::Stream,
    TlsError,
};

/// A wrapper around an underlying raw stream which implements the TLS protocol.
pub type TlsStream<IO> = Stream<IO, ServerConnection>;
/// TlsStream for read only.
pub type TlsStreamReadHalf<IO> = ReadHalf<IO, ServerConnection>;
/// TlsStream for write only.
pub type TlsStreamWriteHalf<IO> = WriteHalf<IO, ServerConnection>;

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
        let mut stream = Stream::new(stream, session);
        stream.handshake().await?;
        Ok(stream)
    }
}
