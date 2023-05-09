use anyhow::{anyhow, bail};
use async_recursion::async_recursion;
use bytes::BytesMut;
use monoio::io::{AsyncReadRent, AsyncWriteRent};
use native_tls::{HandshakeError, MidHandshakeTlsStream};

use crate::{std_adapter::StdAdapter, stream::Stream};

pub(crate) struct HandshakeStream<IO> {
    inner: MidHandshakeTlsStream<StdAdapter<IO>>,
}

impl<IO> HandshakeStream<IO> {
    pub(crate) fn new(stream: MidHandshakeTlsStream<StdAdapter<IO>>) -> Self {
        Self { inner: stream }
    }
}

impl<IO> HandshakeStream<IO>
where
    IO: AsyncReadRent + AsyncWriteRent + 'static,
{
    #[async_recursion(?Send)]
    pub(crate) async fn handshake(self) -> anyhow::Result<Stream<IO>> {
        let mut stream = self.inner;
        if let (Err(e), _) = AsyncReadRent::read(stream.get_mut(), BytesMut::new()).await {
            return Err(anyhow!(e.to_string()));
        }
        match stream.handshake() {
            Ok(stream) => Ok(Stream::new(stream)),
            Err(HandshakeError::WouldBlock(mut stream)) => {
                if let (Err(e), _) = AsyncWriteRent::write(stream.get_mut(), BytesMut::new()).await
                {
                    return Err(anyhow!(e.to_string()));
                }
                HandshakeStream::new(stream).handshake().await
            }
            Err(_) => bail!("tls handshake error"),
        }
    }
}
