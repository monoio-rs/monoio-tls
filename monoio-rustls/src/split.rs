//! Split implement for TlsStream.
//! Note: Here we depends on the behavior of monoio TcpStream.
//! Though it is not a good assumption, it can really make it
//! more efficient with less code. The read and write will not
//! interfere each other.

use std::{
    cell::UnsafeCell,
    future::Future,
    ops::{Deref, DerefMut},
    rc::Rc,
};

use monoio::{
    buf::{IoBuf, IoBufMut, IoVecBuf, IoVecBufMut, RawBuf},
    io::{AsyncReadRent, AsyncWriteRent},
    BufResult,
};
use rustls::{ConnectionCommon, SideData};

use crate::stream::Stream;

#[derive(Debug)]
pub struct ReadHalf<IO, C> {
    pub(crate) inner: Rc<UnsafeCell<Stream<IO, C>>>,
}

#[derive(Debug)]
pub struct WriteHalf<IO, C> {
    pub(crate) inner: Rc<UnsafeCell<Stream<IO, C>>>,
}

impl<IO: AsyncReadRent + AsyncWriteRent, C, SD: SideData> AsyncReadRent for ReadHalf<IO, C>
where
    C: DerefMut + Deref<Target = ConnectionCommon<SD>>,
{
    type ReadFuture<'a, T> = impl Future<Output = BufResult<usize, T>>
    where
        T: IoBufMut + 'a, Self: 'a;

    type ReadvFuture<'a, T> = impl Future<Output = BufResult<usize, T>>
    where
        T: IoVecBufMut + 'a, Self: 'a;

    fn read<T: IoBufMut>(&mut self, buf: T) -> Self::ReadFuture<'_, T> {
        let inner = unsafe { &mut *self.inner.get() };
        inner.read_inner(buf, true)
    }

    fn readv<T: IoVecBufMut>(&mut self, mut buf: T) -> Self::ReadvFuture<'_, T> {
        async move {
            (
                match unsafe { RawBuf::new_from_iovec_mut(&mut buf) } {
                    Some(raw_buf) => self.read(raw_buf).await.0,
                    None => Ok(0),
                },
                buf,
            )
        }
    }
}

impl<IO, C> ReadHalf<IO, C> {
    pub fn reunite(self, other: WriteHalf<IO, C>) -> Result<Stream<IO, C>, ReuniteError<IO, C>> {
        reunite(self, other)
    }
}

impl<IO: AsyncReadRent + AsyncWriteRent, C, SD: SideData> AsyncWriteRent for WriteHalf<IO, C>
where
    C: DerefMut + Deref<Target = ConnectionCommon<SD>>,
{
    type WriteFuture<'a, T> = impl Future<Output = BufResult<usize, T>>
    where
        T: IoBuf + 'a, Self: 'a;

    type WritevFuture<'a, T> = impl Future<Output = BufResult<usize, T>>
    where
        T: IoVecBuf + 'a, Self: 'a;

    type FlushFuture<'a> = impl Future<Output = Result<(), std::io::Error>>
    where
        Self: 'a;

    type ShutdownFuture<'a> = impl Future<Output = Result<(), std::io::Error>>
    where
        Self: 'a;

    fn write<T: IoBuf>(&mut self, buf: T) -> Self::WriteFuture<'_, T> {
        let inner = unsafe { &mut *self.inner.get() };
        inner.write(buf)
    }

    // TODO: use real writev
    fn writev<T: IoVecBuf>(&mut self, buf_vec: T) -> Self::WritevFuture<'_, T> {
        let inner = unsafe { &mut *self.inner.get() };
        inner.writev(buf_vec)
    }

    fn flush(&mut self) -> Self::FlushFuture<'_> {
        let inner = unsafe { &mut *self.inner.get() };
        inner.flush()
    }

    fn shutdown(&mut self) -> Self::ShutdownFuture<'_> {
        let inner = unsafe { &mut *self.inner.get() };
        inner.shutdown()
    }
}

impl<IO, C> WriteHalf<IO, C> {
    pub fn reunite(self, other: ReadHalf<IO, C>) -> Result<Stream<IO, C>, ReuniteError<IO, C>> {
        reunite(other, self)
    }
}

pub(crate) fn reunite<IO, C>(
    read: ReadHalf<IO, C>,
    write: WriteHalf<IO, C>,
) -> Result<Stream<IO, C>, ReuniteError<IO, C>> {
    if Rc::ptr_eq(&read.inner, &write.inner) {
        drop(write);
        // This unwrap cannot fail as the api does not allow creating more than two Rcs,
        // and we just dropped the other half.
        Ok(Rc::try_unwrap(read.inner)
            .expect("TlsStream: try_unwrap failed in reunite")
            .into_inner())
    } else {
        Err(ReuniteError(read, write))
    }
}

/// Error indicating that two halves were not from the same socket, and thus could
/// not be reunited.
#[derive(Debug)]
pub struct ReuniteError<IO, C>(pub ReadHalf<IO, C>, pub WriteHalf<IO, C>);

impl<IO: std::fmt::Debug, C: std::fmt::Debug> std::fmt::Display for ReuniteError<IO, C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "tried to reunite halves that are not from the same socket"
        )
    }
}

impl<IO: std::fmt::Debug, C: std::fmt::Debug> std::error::Error for ReuniteError<IO, C> {}
