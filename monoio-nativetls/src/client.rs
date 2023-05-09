use std::sync::Arc;

use anyhow::anyhow;
use bytes::BytesMut;
use monoio::{io::AsyncWriteRent, net::TcpStream};
use native_tls::HandshakeError;

use crate::{handshake::HandshakeStream, std_adapter::StdAdapter, stream::Stream};

#[derive(Clone)]
pub struct TlsConnector {
    inner: Arc<native_tls::TlsConnector>,
}

impl From<Arc<native_tls::TlsConnector>> for TlsConnector {
    fn from(inner: Arc<native_tls::TlsConnector>) -> TlsConnector {
        TlsConnector { inner }
    }
}

impl From<native_tls::TlsConnector> for TlsConnector {
    fn from(inner: native_tls::TlsConnector) -> TlsConnector {
        TlsConnector {
            inner: Arc::new(inner),
        }
    }
}

impl TlsConnector {
    pub async fn connect(
        &self,
        domain: &str,
        stream: TcpStream,
    ) -> anyhow::Result<Stream<TcpStream>> {
        let stream = StdAdapter::new(stream);
        match self.inner.clone().connect(domain, stream) {
            Ok(stream) => Ok(Stream::new(stream)),
            Err(HandshakeError::WouldBlock(mut stream)) => {
                if let (Err(e), _) = AsyncWriteRent::write(stream.get_mut(), BytesMut::new()).await
                {
                    return Err(anyhow!(e.to_string()));
                }
                HandshakeStream::new(stream).handshake().await
            }
            Err(e) => Err(anyhow!(e.to_string())),
        }
    }
}
