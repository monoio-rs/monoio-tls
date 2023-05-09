use std::{
    future::Future,
    io::{self, Read, Write},
};

use bytes::BytesMut;
use monoio::{
    buf::{IoBuf, IoBufMut, IoVecBuf, IoVecBufMut, RawBuf},
    io::{AsyncReadRent, AsyncWriteRent, Split},
    BufResult,
};
use native_tls::TlsStream;

use crate::std_adapter::StdAdapter;
#[derive(Debug)]
pub struct Stream<IO> {
    pub(crate) io: TlsStream<StdAdapter<IO>>,
}

impl<IO> Stream<IO> {
    pub fn new(io: TlsStream<StdAdapter<IO>>) -> Self {
        Self { io }
    }
}

unsafe impl<IO> Split for Stream<IO> {}

impl<IO> AsyncReadRent for Stream<IO>
where
    IO: AsyncReadRent + AsyncWriteRent,
{
    type ReadFuture<'a, T> = impl Future<Output = BufResult<usize, T>> + 'a
    where
        T: IoBufMut + 'a, Self: 'a;

    type ReadvFuture<'a, T> = impl Future<Output = BufResult<usize, T>> + 'a
    where
        T: IoVecBufMut + 'a, Self: 'a;

    fn read<T: IoBufMut>(&mut self, buf: T) -> Self::ReadFuture<'_, T> {
        let mut buf = buf;
        let slice = unsafe { std::slice::from_raw_parts_mut(buf.write_ptr(), buf.bytes_total()) };
        async move {
            loop {
                match self.io.read(slice) {
                    Ok(n) => {
                        unsafe { buf.set_init(n) };
                        return (Ok(n), buf);
                    }
                    // we need more data, read something.
                    Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => {
                        match AsyncReadRent::read(&mut self.io.get_mut(), BytesMut::new()).await {
                            (Ok(_), _) => continue,
                            (Err(e), _) => return (Err(e), buf),
                        }
                    }
                    Err(e) => {
                        return (Err(e), buf);
                    }
                }
            }
        }
    }

    fn readv<T: IoVecBufMut>(&mut self, mut buf: T) -> Self::ReadvFuture<'_, T> {
        async move {
            let n = match unsafe { RawBuf::new_from_iovec_mut(&mut buf) } {
                Some(raw_buf) => self.read(raw_buf).await.0,
                None => Ok(0),
            };
            if let Ok(n) = n {
                unsafe { buf.set_init(n) };
            }
            (n, buf)
        }
    }
}

impl<IO> AsyncWriteRent for Stream<IO>
where
    IO: AsyncReadRent + AsyncWriteRent,
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
        // construct slice
        let slice = unsafe { std::slice::from_raw_parts(buf.read_ptr(), buf.bytes_init()) };
        async move {
            loop {
                // write slice
                match self.io.write(slice) {
                    Ok(n) => {
                        return (Ok(n), buf);
                    }
                    Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => {
                        match AsyncWriteRent::write(self.io.get_mut(), BytesMut::new()).await {
                            (Ok(_), _) => (),
                            (Err(e), _) => return (Err(e), buf),
                        }
                        continue;
                    }
                    Err(e) => {
                        return (Err(e), buf);
                    }
                };
            }
        }
    }

    // TODO: use real writev
    fn writev<T: IoVecBuf>(&mut self, buf_vec: T) -> Self::WritevFuture<'_, T> {
        async move {
            let n = match unsafe { RawBuf::new_from_iovec(&buf_vec) } {
                Some(raw_buf) => self.write(raw_buf).await.0,
                None => Ok(0),
            };
            (n, buf_vec)
        }
    }

    fn flush(&mut self) -> Self::FlushFuture<'_> {
        AsyncWriteRent::flush(self.io.get_mut())
    }

    fn shutdown(&mut self) -> Self::ShutdownFuture<'_> {
        AsyncWriteRent::shutdown(self.io.get_mut())
    }
}
