#[cfg(not(feature = "unsafe_io"))]
mod safe_io;
#[cfg(not(feature = "unsafe_io"))]
pub use safe_io::{BufferedReader, BufferedWriter};

#[cfg(feature = "unsafe_io")]
mod unsafe_io;
#[cfg(feature = "unsafe_io")]
pub use unsafe_io::{BufferedReader, BufferedWriter};
