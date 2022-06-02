#![feature(generic_associated_types)]
#![feature(type_alias_impl_trait)]

mod client;
mod error;
mod server;
mod split;
mod stream;
mod unsafe_io;

pub use client::{
    TlsConnector, TlsStream as ClientTlsStream, TlsStreamReadHalf as ClientTlsStreamReadHalf,
    TlsStreamWriteHalf as ClientTlsStreamWriteHalf,
};
pub use error::TlsError;
pub use server::{
    TlsAcceptor, TlsStream as ServerTlsStream, TlsStreamReadHalf as ServerTlsStreamReadHalf,
    TlsStreamWriteHalf as ServerTlsStreamWriteHalf,
};
