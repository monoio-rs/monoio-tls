use std::sync::Arc;

use monoio::io::{AsyncReadRent, AsyncWriteRent};
use rustls::{ClientConfig, ClientConnection, Connection};

use crate::{stream::Stream, TlsError};

/// A wrapper around an underlying raw stream which implements the TLS protocol.
pub type TlsStream<IO> = Stream<IO>;

/// A wrapper around a `rustls::ClientConfig`, providing an async `connect` method.
#[derive(Clone)]
pub struct TlsConnector {
    inner: Arc<ClientConfig>,
}

impl From<Arc<ClientConfig>> for TlsConnector {
    fn from(inner: Arc<ClientConfig>) -> TlsConnector {
        TlsConnector { inner }
    }
}

impl From<ClientConfig> for TlsConnector {
    fn from(inner: ClientConfig) -> TlsConnector {
        TlsConnector {
            inner: Arc::new(inner),
        }
    }
}

impl TlsConnector {
    pub async fn connect<IO>(
        &self,
        domain: rustls::ServerName,
        stream: IO,
    ) -> Result<TlsStream<IO>, TlsError>
    where
        IO: AsyncReadRent + AsyncWriteRent,
    {
        let session = ClientConnection::new(self.inner.clone(), domain)?;
        let mut stream = Stream::new(stream, Connection::Client(session));
        stream.handshake().await?;
        Ok(stream)
    }
}
