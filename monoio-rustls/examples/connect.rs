use std::sync::Arc;

use monoio::{
    io::{AsyncReadRent, AsyncWriteRentExt},
    net::TcpStream,
};
use monoio_rustls::TlsConnector;
use rustls::{RootCertStore};

#[monoio::main]
async fn main() {
    let mut root_store = RootCertStore::empty();
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();

    let connector = TlsConnector::from(Arc::new(config));
    let stream = TcpStream::connect("rsproxy.cn:443").await.unwrap();
    println!("rsproxy.cn:443 connected");

    let domain = rustls::pki_types::ServerName::try_from("rsproxy.cn").unwrap();
    let mut stream = connector.connect(domain, stream).await.unwrap();
    println!("handshake success");

    let content = b"GET / HTTP/1.0\r\nHost: rsproxy.cn\r\n\r\n";
    let (r, _) = stream.write_all(content).await;
    r.expect("unable to write http request");
    println!("http request sent");

    let buf = vec![0_u8; 64];
    let (r, buf) = stream.read(buf).await;
    r.expect("unable to read http response");
    let resp = String::from_utf8(buf).unwrap();
    println!("http response recv: \n\n{}", resp);
}
