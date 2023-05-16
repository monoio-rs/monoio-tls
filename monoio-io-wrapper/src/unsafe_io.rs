use std::{io, mem};

use monoio::{
    buf::{IoBuf, IoBufMut},
    io::{AsyncReadRent, AsyncWriteRent},
};

/// Used by both UnsafeRead and UnsafeWrite.
#[derive(Debug)]
enum Status {
    /// We haven't do real io, and maybe the dest is recorded.
    WaitFill(Option<(*const u8, usize)>),
    /// We have already do real io. The length maybe zero or non-zero.
    Filled(Result<usize, io::Error>),
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
#[derive(Default, Debug)]
pub struct UnsafeRead {
    status: Status,
}

impl UnsafeRead {
    pub const fn new() -> Self {
        Self {
            status: Status::WaitFill(None),
        }
    }

    /// `do_io` must be called after calling to io::Read::read.
    /// # Handle return value
    /// 1. Ok(n): previous read data length.
    /// 2. Err(e): previous error.
    /// 3. Err(WouldBlock): need calling read to capture ptr and len.
    /// # Safety
    /// User must make sure the former buffer is still valid until io finished.
    pub async unsafe fn do_io<IO: AsyncReadRent>(&mut self, mut io: IO) -> io::Result<usize> {
        match self.status {
            Status::WaitFill(Some((ptr, len))) => {
                let buf = RawBuf { ptr, len };
                let read_ret = io.read(buf).await.0;
                let rret: io::Result<usize> = match &read_ret {
                    Ok(n) => Ok(*n),
                    Err(e) => Err(e.kind().into()),
                };
                self.status = Status::Filled(read_ret);
                rret
            }
            Status::WaitFill(None) => Err(io::ErrorKind::WouldBlock.into()),
            Status::Filled(ref prev_ret) => match prev_ret {
                Ok(n) => Ok(*n),
                Err(e) => Err(e.kind().into()),
            },
        }
    }

    /// Clear status.
    pub fn reset(&mut self) {
        self.status = Status::WaitFill(None);
    }
}

impl io::Read for UnsafeRead {
    /// `read` from buffer(not really a buffer).
    /// # Handle return value
    /// 1. Err(WouldBlock): ptr and len captured, need call do_io and then retry read.
    /// 2. _: handle it.
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match mem::replace(&mut self.status, Status::WaitFill(None)) {
            Status::WaitFill(_) => {
                let ptr = buf.as_ptr();
                let len = buf.len();
                self.status = Status::WaitFill(Some((ptr, len)));
                Err(io::ErrorKind::WouldBlock.into())
            }
            Status::Filled(ret) => ret,
        }
    }
}

/// UnsafeWrite behaves like `UnsafeRead`.
#[derive(Default, Debug)]
pub struct UnsafeWrite {
    status: Status,
}

impl UnsafeWrite {
    pub const fn new() -> Self {
        Self {
            status: Status::WaitFill(None),
        }
    }

    /// `do_io` must be called after calling to io::Write::write.
    /// # Handle return value
    /// 1. Ok(n): previous written data length.
    /// 2. Err(e): previous error.
    /// 3. Err(WouldBlock): need calling write to capture ptr and len.
    /// # Safety
    /// User must make sure the former buffer is still valid until io finished.
    pub async unsafe fn do_io<IO: AsyncWriteRent>(&mut self, mut io: IO) -> io::Result<usize> {
        match self.status {
            Status::WaitFill(Some((ptr, len))) => {
                let buf = RawBuf { ptr, len };
                let write_ret = io.write(buf).await.0;
                let rret: io::Result<usize> = match &write_ret {
                    Ok(n) => Ok(*n),
                    Err(e) => Err(e.kind().into()),
                };
                self.status = Status::Filled(write_ret);
                rret
            }
            Status::WaitFill(None) => Err(io::ErrorKind::WouldBlock.into()),
            Status::Filled(ref prev_ret) => match prev_ret {
                Ok(n) => Ok(*n),
                Err(e) => Err(e.kind().into()),
            },
        }
    }

    /// Clear status.
    pub fn reset(&mut self) {
        self.status = Status::WaitFill(None);
    }
}

impl io::Write for UnsafeWrite {
    /// `read` to buffer.
    /// # Handle return value
    /// 1. Err(WouldBlock): ptr and len captured, need call do_io and then retry write.
    /// 2. _: handle it.
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match mem::replace(&mut self.status, Status::WaitFill(None)) {
            Status::WaitFill(_) => {
                let ptr = buf.as_ptr();
                let len = buf.len();
                self.status = Status::WaitFill(Some((ptr, len)));
                Err(io::ErrorKind::WouldBlock.into())
            }
            Status::Filled(ret) => ret,
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

    fn bytes_total(&mut self) -> usize {
        self.len
    }

    unsafe fn set_init(&mut self, _pos: usize) {}
}
