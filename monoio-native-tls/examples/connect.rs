use monoio::{
    io::{AsyncReadRent, AsyncWriteRentExt},
    net::TcpStream,
};
use monoio_native_tls::TlsConnector;
use native_tls::TlsConnector as NativeTlsConnector;

#[monoio::main]
async fn main() {
    let connector = NativeTlsConnector::builder().build().unwrap();
    let connector = TlsConnector::from(connector);

    let stream = TcpStream::connect("rsproxy.cn:443").await.unwrap();
    println!("rsproxy.cn:443 connected");

    let mut stream = connector.connect("rsproxy.cn", stream).await.unwrap();
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
