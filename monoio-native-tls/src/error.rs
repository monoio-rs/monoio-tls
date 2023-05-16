use std::io;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum TlsError {
    #[error("io error")]
    Io(#[from] std::io::Error),
    #[error("native-tls error")]
    NativeTls(#[from] native_tls::Error),
}

impl From<TlsError> for io::Error {
    fn from(e: TlsError) -> Self {
        match e {
            TlsError::Io(e) => e,
            TlsError::NativeTls(e) => io::Error::new(io::ErrorKind::Other, e),
        }
    }
}
