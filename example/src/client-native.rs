use monoio::{
    io::{AsyncReadRentExt, AsyncWriteRent, AsyncWriteRentExt},
    net::TcpStream,
};
use monoio_native_tls::TlsConnector;
use native_tls::Certificate;

#[monoio::main]
async fn main() {
    let raw_connector = native_tls::TlsConnector::builder()
        .add_root_certificate(read_ca_certs())
        .build()
        .unwrap();
    let connector = TlsConnector::from(raw_connector);
    let stream = TcpStream::connect("127.0.0.1:50443").await.unwrap();
    println!("127.0.0.1:50443 connected");

    let mut stream = connector.connect("monoio.rs", stream).await.unwrap();
    println!("handshake success");

    let data = "hello world";
    stream.write_all(data).await.0.expect("unable to send data");
    println!("send data: {data}");
    let buf = vec![0; data.len()];
    let (res, buf) = stream.read_exact(buf).await;
    assert!(res.is_ok(), "unable to recv data");
    println!(
        "recv data: {}",
        String::from_utf8(buf).expect("invalid data")
    );
    let _ = stream.shutdown().await;
}

fn read_ca_certs() -> Certificate {
    Certificate::from_pem(include_bytes!("../certs/rootCA.crt")).unwrap()
}
