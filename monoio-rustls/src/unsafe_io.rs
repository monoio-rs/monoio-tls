use std::io;

use monoio::{buf::{IoBufMut, IoBuf}, io::{AsyncReadRent, AsyncWriteRent}};

/// Used by both UnsafeRead and UnsafeWrite.
enum Status {
    /// We haven't do real io, and maybe the dest is recorded.
    WaitFill(Option<(*const u8, usize)>),
    /// We have already do real io. The length maybe zero or non-zero.
    Filled(usize),
}

impl Default for Status {
    fn default() -> Self {
        Status::WaitFill(None)
    }
}

/// UnsafeRead is a wrapper of some meta data.
/// It implements std::io::Read trait. But it do real io in an async way.
/// On the first read, it may returns WouldBlock error, which means the
/// `fullfill` should be called to do real io.
/// The data is read directly into the dest that last std read passes.
/// Note that this action is an unsafe hack to avoid data copy.
/// You can only use this wrapper when you make sure the read dest is always
/// a valid buffer.
#[derive(Default)]
pub(crate) struct UnsafeRead {
    status: Status,
}

impl UnsafeRead {
    /// `do_io` must be called after calling to io::Read::read.
    pub(crate) async unsafe fn do_io<IO: AsyncReadRent>(&mut self, mut io: IO) -> io::Result<usize> {
        match self.status {
            Status::WaitFill(Some((ptr, len))) => {
                let buf = RawBuf { ptr, len };
                let n = io.read(buf).await.0?;
                self.status = Status::Filled(n);
                Ok(n)
            }
            Status::Filled(len) => Ok(len),
            Status::WaitFill(None) => Err(io::ErrorKind::WouldBlock.into()),
        }
    }
}

impl io::Read for UnsafeRead {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self.status {
            Status::WaitFill(_) => {
                let ptr = buf.as_ptr();
                let len = buf.len();
                self.status = Status::WaitFill(Some((ptr, len)));
                Err(io::ErrorKind::WouldBlock.into())
            }
            Status::Filled(len) => {
                if len != 0 {
                    // reset only when not eof
                    self.status = Status::WaitFill(None);
                }
                Ok(len)
            }
        }
    }
}

/// UnsafeWrite behaves like `UnsafeRead`.
#[derive(Default)]
pub(crate) struct UnsafeWrite {
    status: Status,
}

impl UnsafeWrite {
    /// `do_io` must be called after calling to io::Write::write.
    pub(crate) async unsafe fn do_io<IO: AsyncWriteRent>(&mut self, mut io: IO) -> io::Result<usize> {
        match self.status {
            Status::WaitFill(Some((ptr, len))) => {
                let buf = RawBuf { ptr, len };
                let n = io.write(buf).await.0?;
                self.status = Status::Filled(n);
                Ok(n)
            }
            Status::Filled(len) => Ok(len),
            Status::WaitFill(None) => Err(io::ErrorKind::WouldBlock.into()),
        }
    }
}

impl io::Write for UnsafeWrite {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self.status {
            Status::WaitFill(_) => {
                let ptr = buf.as_ptr();
                let len = buf.len();
                self.status = Status::WaitFill(Some((ptr, len)));
                Err(io::ErrorKind::WouldBlock.into())
            }
            Status::Filled(len) => {
                if len != 0 {
                    // reset only when not eof
                    self.status = Status::WaitFill(None);
                }
                Ok(len)
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// RawBuf is a wrapper of buffer ptr and len.
/// It seems that it hold the ownership of the buffer, which infact not.
/// Use this wrapper only when you can make sure the buffer ptr lives
/// longer than the wrapper.
struct RawBuf {
    ptr: *const u8,
    len: usize,
}

unsafe impl IoBuf for RawBuf {
    fn read_ptr(&self) -> *const u8 {
        self.ptr
    }

    fn bytes_init(&self) -> usize {
        self.len
    }
}

unsafe impl IoBufMut for RawBuf {
    fn write_ptr(&mut self) -> *mut u8 {
        self.ptr as *mut u8
    }

    fn bytes_total(&self) -> usize {
        self.len
    }

    unsafe fn set_init(&mut self, _pos: usize) {}
}