use std::sync::Arc;

use monoio::io::{AsyncReadRent, AsyncWriteRent, OwnedReadHalf, OwnedWriteHalf};
use rustls::{ServerConfig, ServerConnection};

use crate::{stream::Stream, TlsError};

/// A wrapper around an underlying raw stream which implements the TLS protocol.
pub type TlsStream<IO> = Stream<IO, ServerConnection>;
/// TlsStream for read only.
pub type TlsStreamReadHalf<IO> = OwnedReadHalf<TlsStream<IO>>;
/// TlsStream for write only.
pub type TlsStreamWriteHalf<IO> = OwnedWriteHalf<TlsStream<IO>>;

/// A wrapper around a `rustls::ServerConfig`, providing an async `accept` method.
#[derive(Clone)]
pub struct TlsAcceptor {
    inner: Arc<ServerConfig>,
    #[cfg(feature = "unsafe_io")]
    unsafe_io: bool,
}

impl From<Arc<ServerConfig>> for TlsAcceptor {
    fn from(inner: Arc<ServerConfig>) -> TlsAcceptor {
        TlsAcceptor {
            inner,
            #[cfg(feature = "unsafe_io")]
            unsafe_io: false,
        }
    }
}

impl From<ServerConfig> for TlsAcceptor {
    fn from(inner: ServerConfig) -> TlsAcceptor {
        TlsAcceptor {
            inner: Arc::new(inner),
            #[cfg(feature = "unsafe_io")]
            unsafe_io: false,
        }
    }
}

impl TlsAcceptor {
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

    pub async fn accept<IO>(&self, stream: IO) -> Result<TlsStream<IO>, TlsError>
    where
        IO: AsyncReadRent + AsyncWriteRent,
    {
        let session = ServerConnection::new(self.inner.clone())?;
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
