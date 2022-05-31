use std::{future::Future, sync::Arc};

use monoio::{
    io::{AsyncReadRent, AsyncWriteRent},
    BufResult,
};
use rustls::{ServerConfig, ServerConnection};

use crate::{stream::Stream, TlsError};

/// A wrapper around an underlying raw stream which implements the TLS protocol.
#[derive(Debug)]
pub struct TlsStream<IO>(pub(crate) Stream<IO, ServerConnection>);

impl<IO> AsyncReadRent for TlsStream<IO>
where
    IO: AsyncReadRent + AsyncWriteRent,
{
    type ReadFuture<'a, T> = impl Future<Output = BufResult<usize, T>>
    where
        T: 'a, IO: 'a;

    type ReadvFuture<'a, T> = impl Future<Output = BufResult<usize, T>>
    where
        T: 'a, IO: 'a;

    fn read<T: monoio::buf::IoBufMut>(&mut self, buf: T) -> Self::ReadFuture<'_, T> {
        self.0.read(buf)
    }

    fn readv<T: monoio::buf::IoVecBufMut>(&mut self, buf: T) -> Self::ReadvFuture<'_, T> {
        self.0.readv(buf)
    }
}

impl<IO> AsyncWriteRent for TlsStream<IO>
where
    IO: AsyncReadRent + AsyncWriteRent,
{
    type WriteFuture<'a, T> = impl Future<Output = BufResult<usize, T>>
    where
        T: 'a, IO: 'a;

    type WritevFuture<'a, T> = impl Future<Output = BufResult<usize, T>>
    where
        T: 'a, IO: 'a;

    type ShutdownFuture<'a> = impl Future<Output = Result<(), std::io::Error>>
    where
        IO: 'a;

    fn write<T: monoio::buf::IoBuf>(&mut self, buf: T) -> Self::WriteFuture<'_, T> {
        self.0.write(buf)
    }

    fn writev<T: monoio::buf::IoVecBuf>(&mut self, buf_vec: T) -> Self::WritevFuture<'_, T> {
        self.0.writev(buf_vec)
    }

    fn shutdown(&mut self) -> Self::ShutdownFuture<'_> {
        self.0.shutdown()
    }
}

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
        Ok(TlsStream(stream))
    }
}
