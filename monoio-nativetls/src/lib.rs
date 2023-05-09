#![allow(stable_features)]
#![feature(impl_trait_in_assoc_type)]

mod client;
mod ffi;
mod handshake;
mod server;
mod std_adapter;
mod stream;

pub type TlsStream<S> = stream::Stream<S>;

#[cfg(feature = "qat")]
use std::sync::Once;

pub use client::TlsConnector;
pub use server::TlsAcceptor;

#[cfg(feature = "qat")]
const INIT_ONCE: Once = Once::new();
#[cfg(feature = "qat")]
const LKCF_ENGINE: &[u8] = b"lkcf-engine\0";

#[cfg(feature = "qat")]
pub fn init() {
    use ffi::init_openssl_engine;

    INIT_ONCE.call_once(|| {
        init_openssl_engine(unsafe { std::ffi::CStr::from_bytes_with_nul_unchecked(LKCF_ENGINE) })
    });
}

#[cfg(not(feature = "qat"))]
pub fn init() {}
