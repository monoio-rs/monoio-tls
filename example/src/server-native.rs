//! An echo server with tls.
#![feature(concat_bytes)]

use std::io;

use monoio::{
    io::{AsyncReadRent, AsyncWriteRentExt},
    net::{TcpListener, TcpStream},
};
use monoio_native_tls::TlsAcceptor;
use native_tls::Identity;

#[monoio::main]
async fn main() {
    let raw_acceptor = native_tls::TlsAcceptor::new(read_server_certs()).unwrap();
    let acceptor = TlsAcceptor::from(raw_acceptor);

    let listener = TcpListener::bind("127.0.0.1:50443").expect("unable to listen 127.0.0.1:50443");
    while let Ok((stream, addr)) = listener.accept().await {
        println!("Accepted from {addr}, will accept tls handshake");
        let tls_acceptor = acceptor.clone();
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
        }
        Err(e) => {
            println!("Unable to do handshake: {e}");
            return Err(e.into());
        }
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

fn read_server_certs() -> Identity {
    let chain = concat_bytes!(
        include_bytes!("../certs/server.crt"),
        include_bytes!("../certs/rootCA.crt")
    );
    Identity::from_pkcs8(&chain[..], include_bytes!("../certs/server.pkcs8")).unwrap()
}
