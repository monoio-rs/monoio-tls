#![allow(stable_features)]
#![feature(generic_associated_types)]
#![feature(impl_trait_in_assoc_type)]
#![feature(type_alias_impl_trait)]

mod client;
mod error;
mod server;
mod stream;

pub use client::{TlsConnector, TlsStream as ClientTlsStream};
pub use error::TlsError;
pub use server::{TlsAcceptor, TlsStream as ServerTlsStream};
