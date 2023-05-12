#![feature(impl_trait_in_assoc_type)]
#![feature(type_alias_impl_trait)]

mod client;
mod error;
mod server;
mod stream;
mod utils;

pub use client::TlsConnector;
pub use error::TlsError;
pub use server::TlsAcceptor;
pub use stream::TlsStream;

#[cfg(feature = "qat")]
mod ffi;

pub fn init() {
    #[cfg(feature = "qat")]
    static INIT_ONCE: std::sync::Once = std::sync::Once::new();
    #[cfg(feature = "qat")]
    const LKCF_ENGINE: &[u8] = b"lkcf-engine\0";

    #[cfg(feature = "qat")]
    INIT_ONCE.call_once(|| {
        ffi::init_openssl_engine(unsafe {
            std::ffi::CStr::from_bytes_with_nul_unchecked(LKCF_ENGINE)
        })
    });
}
