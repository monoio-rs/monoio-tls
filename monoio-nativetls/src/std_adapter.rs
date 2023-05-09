use std::{future::Future, io};

use monoio::{
    buf::{IoBuf, IoBufMut, IoVecBuf, IoVecBufMut},
    io::{AsyncReadRent, AsyncWriteRent},
    BufResult,
};
use monoio_common::{BufferedReader, BufferedWriter};

#[derive(Debug)]
pub struct StdAdapter<S> {
    inner: S,
    r_buffer: BufferedReader,
    w_buffer: BufferedWriter,
    w_state: ProcessState,
}

#[derive(Debug)]
enum ProcessState {
    Unknown,
    Pending(usize, usize),
    Proceed(usize),
}

impl Default for ProcessState {
    fn default() -> Self {
        Self::Unknown
    }
}

impl<S> StdAdapter<S> {
    pub(crate) fn new(inner: S) -> Self {
        StdAdapter {
            inner,
            w_state: ProcessState::default(),
            r_buffer: Default::default(),
            w_buffer: Default::default(),
        }
    }

    pub(crate) fn into_parts(&mut self) -> (&mut S, &mut BufferedReader, &mut BufferedWriter) {
        (&mut self.inner, &mut self.r_buffer, &mut self.w_buffer)
    }
}

impl<S> AsyncReadRent for StdAdapter<S>
where
    S: AsyncReadRent + AsyncWriteRent,
{
    type ReadFuture<'a, T> = impl Future<Output = BufResult<usize, T>> + 'a
    where
        T: IoBufMut + 'a, Self: 'a;

    type ReadvFuture<'a, T> = impl Future<Output = BufResult<usize, T>> + 'a
    where
        T: IoVecBufMut + 'a, Self: 'a;

    fn read<T: monoio::buf::IoBufMut>(&mut self, buf: T) -> Self::ReadFuture<'_, T> {
        let (io, r_buffer, _) = self.into_parts();
        async {
            #[allow(unused)]
            let result = unsafe { r_buffer.do_io(io) }.await;
            (result, buf)
        }
    }

    fn readv<T: monoio::buf::IoVecBufMut>(&mut self, _buf: T) -> Self::ReadvFuture<'_, T> {
        async { unimplemented!("should not use") }
    }
}

impl<S> AsyncWriteRent for StdAdapter<S>
where
    S: AsyncReadRent + AsyncWriteRent,
{
    type WriteFuture<'a, T> = impl Future<Output = BufResult<usize, T>> + 'a
    where
        T: IoBuf + 'a, Self: 'a;

    type WritevFuture<'a, T> = impl Future<Output = BufResult<usize, T>> + 'a
    where
        T: IoVecBuf + 'a, Self: 'a;

    type FlushFuture<'a> = impl Future<Output = io::Result<()>> + 'a
    where
        Self: 'a;

    type ShutdownFuture<'a> = impl Future<Output = io::Result<()>> + 'a
    where
        Self: 'a;

    fn write<T: IoBuf>(&mut self, buf: T) -> Self::WriteFuture<'_, T> {
        async {
            match self.w_state {
                ProcessState::Unknown => (Ok(0), buf),
                ProcessState::Pending(t, c) => {
                    if c >= t {
                        self.w_state = ProcessState::Proceed(c);
                        return (Ok(c), buf);
                    }
                    let (io, _, w_buffer) = self.into_parts();
                    #[allow(unused)]
                    match unsafe { w_buffer.do_io(io) }.await {
                        Ok(n) => {
                            self.w_state = ProcessState::Pending(t, c + n);
                            (Ok(n), buf)
                        }
                        Err(e) => (Err(e), buf),
                    }
                }
                ProcessState::Proceed(_) => return (Ok(0), buf),
            }
        }
    }

    fn writev<T: IoVecBuf>(&mut self, _buf_vec: T) -> Self::WritevFuture<'_, T> {
        async { unimplemented!("should not use") }
    }

    fn flush(&mut self) -> Self::FlushFuture<'_> {
        self.inner.flush()
    }

    fn shutdown(&mut self) -> Self::ShutdownFuture<'_> {
        self.inner.shutdown()
    }
}

impl<S> io::Read for StdAdapter<S>
where
    S: AsyncReadRent + AsyncWriteRent,
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.r_buffer.read(buf)
    }
}

impl<S> io::Write for StdAdapter<S>
where
    S: AsyncReadRent + AsyncWriteRent,
{
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self.w_state {
            ProcessState::Unknown => match self.w_buffer.write(buf) {
                Ok(n) => {
                    self.w_state = ProcessState::Pending(n, 0);
                    return Err(io::ErrorKind::WouldBlock.into());
                }
                Err(e) => Err(e),
            },
            ProcessState::Pending(_, _) => Err(io::ErrorKind::WouldBlock.into()),
            ProcessState::Proceed(n) => {
                self.w_state = ProcessState::default();
                Ok(n)
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        self.w_buffer.flush()
    }
}
