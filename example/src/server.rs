// An echo server with tls.

use std::io::{Cursor, self};

use monoio::{net::{TcpListener, TcpStream}, io::{AsyncReadRent, AsyncWriteRentExt}};
use monoio_rustls::TlsAcceptor;
use rustls::{Certificate, PrivateKey, ServerConfig};
use rustls_pemfile::{certs, rsa_private_keys};

#[monoio::main]
async fn main() {
    let (chain, key) = read_server_certs();
    let config = ServerConfig::builder()
        .with_safe_defaults()
        .with_no_client_auth()
        .with_single_cert(chain, key)
        .expect("invalid cert chain or key");
    let tls_acceptor = TlsAcceptor::from(config);

    let listener = TcpListener::bind("127.0.0.1:50443").expect("unable to listen 127.0.0.1:50443");
    while let Ok((stream, addr)) = listener.accept().await {
        println!("Accepted from {addr}, will accept tls handshake");
        let tls_acceptor = tls_acceptor.clone();
        monoio::spawn(async move {
            let e = process_raw_stream(stream, tls_acceptor).await;
            println!("Relay finished {e:?}");
        });
    }
    println!("Server exit");
}

async fn process_raw_stream(stream: TcpStream, tls_acceptor: TlsAcceptor) -> io::Result<()> {
    let mut tls_stream = match tls_acceptor.accept(stream).await {
        Ok(s) => {
            println!("Handshake finished, will relay data");
            s
        },
        Err(e) => {
            println!("Unable to do handshake: {e}");
            return Err(e.into());
        },
    };

    let mut n = 0;
    let mut buf = Vec::with_capacity(8 * 1024);
    loop {
        // read
        let (res, _buf) = tls_stream.read(buf).await;
        buf = _buf;
        let res: usize = res?;
        if res == 0 {
            // eof
            break;
        }

        // write all
        let (res, _buf) = tls_stream.write_all(buf).await;
        buf = _buf;
        n += res?;
    }

    println!("Relay finished normally, {n} bytes processed");
    Ok(())
}

fn read_server_certs() -> (Vec<Certificate>, PrivateKey) {
    let mut ca_cursor = Cursor::new(include_bytes!("../certs/rootCA.crt"));
    let ca_data = certs(&mut ca_cursor).unwrap().pop().unwrap();
    let ca = Certificate(ca_data);

    let mut crt_cursor = Cursor::new(include_bytes!("../certs/server.crt"));
    let crt_data = certs(&mut crt_cursor).unwrap().pop().unwrap();
    let crt = Certificate(crt_data);

    let mut key_cursor = Cursor::new(include_bytes!("../certs/server.key"));
    let key_data = rsa_private_keys(&mut key_cursor).unwrap().pop().unwrap();
    let key = PrivateKey(key_data);

    let chain = vec![crt, ca];
    (chain, key)
}
