#![feature(generic_associated_types)]
#![feature(type_alias_impl_trait)]

mod client;
mod error;
mod server;
mod stream;
mod unsafe_io;

pub use client::{TlsConnector, TlsStream as ClientTlsStream};
pub use error::TlsError;
pub use server::{TlsAcceptor, TlsStream as ServerTlsStream};
