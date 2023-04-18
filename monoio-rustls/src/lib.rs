#![allow(stable_features)]
#![feature(generic_associated_types)]
#![feature(impl_trait_in_assoc_type)]
#![feature(type_alias_impl_trait)]

mod client;
mod error;
#[cfg(not(feature = "unsafe_io"))]
mod safe_io;
mod server;
mod split;
mod stream;
#[cfg(feature = "unsafe_io")]
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
