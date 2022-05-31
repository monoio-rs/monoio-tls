# Monoio TLS
TLS Stream Wrapper for Monoio.

## TLS with rustls
`monoio-rustls` provides a TLS stream wrapper. It implements the `monoio::io::AsyncReadRent` and `monoio::io::AsyncWriteRent` trait.

Read `example/src/client.rs` and `example/src/server.rs` for more details.

## TLS with native tls
Maybe todo.

## Licenses
Monoio-tls is licensed under the MIT license or Apache license.

During developing we referenced a lot from tokio-rustls. We would like to thank the authors of the project.