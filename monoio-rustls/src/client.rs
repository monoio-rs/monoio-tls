use std::sync::Arc;

use monoio::io::{AsyncReadRent, AsyncWriteRent, OwnedReadHalf, OwnedWriteHalf};
use rustls::{pki_types::ServerName, ClientConfig, ClientConnection};

use crate::{stream::Stream, TlsError};

/// A wrapper around an underlying raw stream which implements the TLS protocol.
pub type TlsStream<IO> = Stream<IO, ClientConnection>;
/// TlsStream for read only.
pub type TlsStreamReadHalf<IO> = OwnedReadHalf<TlsStream<IO>>;
/// TlsStream for write only.
pub type TlsStreamWriteHalf<IO> = OwnedWriteHalf<TlsStream<IO>>;

/// A wrapper around a `rustls::ClientConfig`, providing an async `connect` method.
#[derive(Clone)]
pub struct TlsConnector {
    inner: Arc<ClientConfig>,
    #[cfg(feature = "unsafe_io")]
    unsafe_io: bool,
}

impl From<Arc<ClientConfig>> for TlsConnector {
    fn from(inner: Arc<ClientConfig>) -> TlsConnector {
        TlsConnector {
            inner,
            #[cfg(feature = "unsafe_io")]
            unsafe_io: false,
        }
    }
}

impl From<ClientConfig> for TlsConnector {
    fn from(inner: ClientConfig) -> TlsConnector {
        TlsConnector {
            inner: Arc::new(inner),
            #[cfg(feature = "unsafe_io")]
            unsafe_io: false,
        }
    }
}

impl TlsConnector {
    /// Enable unsafe-io.
    /// # Safety
    /// Users must make sure the buffer ptr and len is valid until io finished.
    /// So the Future cannot be dropped directly. Consider using CancellableIO.
    #[cfg(feature = "unsafe_io")]
    pub unsafe fn unsafe_io(self, enabled: bool) -> Self {
        Self {
            unsafe_io: enabled,
            ..self
        }
    }

    pub async fn connect<IO>(
        &self,
        domain: ServerName<'static>,
        stream: IO,
    ) -> Result<TlsStream<IO>, TlsError>
    where
        IO: AsyncReadRent + AsyncWriteRent,
    {
        let session = ClientConnection::new(self.inner.clone(), domain)?;
        #[cfg(feature = "unsafe_io")]
        let mut stream = if self.unsafe_io {
            // # Safety
            // Users already maked unsafe io.
            unsafe { Stream::new_unsafe(stream, session) }
        } else {
            Stream::new(stream, session)
        };
        #[cfg(not(feature = "unsafe_io"))]
        let mut stream = Stream::new(stream, session);
        stream.handshake().await?;
        Ok(stream)
    }
}
