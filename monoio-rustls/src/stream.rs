use std::{
    cell::UnsafeCell,
    future::Future,
    io::{self, Read, Write},
    ops::{Deref, DerefMut},
    rc::Rc,
};

use monoio::{
    buf::{IoBuf, IoBufMut, IoVecBuf, IoVecBufMut, RawBuf},
    io::{AsyncReadRent, AsyncWriteRent},
    BufResult,
};
use rustls::{ConnectionCommon, SideData};

use crate::split::{ReadHalf, WriteHalf};

#[derive(Debug)]
pub struct Stream<IO, C> {
    pub(crate) io: IO,
    pub(crate) session: C,
}

impl<IO, C> Stream<IO, C> {
    pub fn new(io: IO, session: C) -> Self {
        Self { io, session }
    }

    pub fn split(self) -> (ReadHalf<IO, C>, WriteHalf<IO, C>) {
        let shared = Rc::new(UnsafeCell::new(self));
        (
            ReadHalf {
                inner: shared.clone(),
            },
            WriteHalf { inner: shared },
        )
    }

    pub fn into_parts(self) -> (IO, C) {
        (self.io, self.session)
    }
}

impl<IO: AsyncReadRent + AsyncWriteRent, C, SD: SideData> Stream<IO, C>
where
    C: DerefMut + Deref<Target = ConnectionCommon<SD>>,
{
    pub(crate) async fn read_io(&mut self, splitted: bool) -> io::Result<usize> {
        #[cfg(feature = "unsafe_io")]
        let mut reader = crate::unsafe_io::UnsafeRead::default();
        #[cfg(not(feature = "unsafe_io"))]
        let mut reader = crate::safe_io::SafeRead::default();

        let n = loop {
            match self.session.read_tls(&mut reader) {
                Ok(n) => {
                    break n;
                }
                Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => {
                    #[allow(unused_unsafe)]
                    unsafe {
                        reader.do_io(&mut self.io).await?
                    };
                    continue;
                }
                Err(err) => return Err(err),
            }
        };

        let state = match self.session.process_new_packets() {
            Ok(state) => state,
            Err(err) => {
                // When to write_io? If we do this in read call, the UnsafeWrite may crash
                // when we impl split in an UnsafeCell way.
                // Here we choose not to do write when read.
                // User should manually shutdown it on error.
                if !splitted {
                    let _ = self.write_io().await;
                }
                return Err(io::Error::new(io::ErrorKind::InvalidData, err));
            }
        };

        if state.peer_has_closed() && self.session.is_handshaking() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "tls handshake alert",
            ));
        }

        Ok(n)
    }

    pub(crate) async fn write_io(&mut self) -> io::Result<usize> {
        #[cfg(feature = "unsafe_io")]
        let mut writer = crate::unsafe_io::UnsafeWrite::default();
        #[cfg(not(feature = "unsafe_io"))]
        let mut writer = crate::safe_io::SafeWrite::default();

        let n = loop {
            match self.session.write_tls(&mut writer) {
                Ok(n) => {
                    break n;
                }
                Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => {
                    #[allow(unused_unsafe)]
                    unsafe {
                        writer.do_io(&mut self.io).await?
                    };
                    continue;
                }
                Err(err) => return Err(err),
            }
        };
        // Flush buffered data, only needed for safe_io.
        #[cfg(not(feature = "unsafe_io"))]
        writer.do_io(&mut self.io).await?;

        Ok(n)
    }

    pub(crate) async fn handshake(&mut self) -> io::Result<(usize, usize)> {
        let mut wrlen = 0;
        let mut rdlen = 0;
        let mut eof = false;

        loop {
            while self.session.wants_write() && self.session.is_handshaking() {
                wrlen += self.write_io().await?;
            }
            while !eof && self.session.wants_read() && self.session.is_handshaking() {
                let n = self.read_io(false).await?;
                rdlen += n;
                if n == 0 {
                    eof = true;
                }
            }

            match (eof, self.session.is_handshaking()) {
                (true, true) => {
                    let err = io::Error::new(io::ErrorKind::UnexpectedEof, "tls handshake eof");
                    return Err(err);
                }
                (false, true) => (),
                (_, false) => {
                    break;
                }
            };
        }

        // flush buffer
        while self.session.wants_write() {
            wrlen += self.write_io().await?;
        }

        Ok((rdlen, wrlen))
    }

    pub(crate) async fn read_inner<T: monoio::buf::IoBufMut>(
        &mut self,
        mut buf: T,
        splitted: bool,
    ) -> BufResult<usize, T> {
        let slice = unsafe { std::slice::from_raw_parts_mut(buf.write_ptr(), buf.bytes_total()) };
        loop {
            // read from rustls to buffer
            match self.session.reader().read(slice) {
                Ok(n) => {
                    unsafe { buf.set_init(n) };
                    return (Ok(n), buf);
                }
                // we need more data, read something.
                Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => (),
                Err(e) => {
                    return (Err(e), buf);
                }
            }

            // now we need data, read something into rustls
            match self.read_io(splitted).await {
                Ok(0) => {
                    return (
                        Err(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "tls raw stream eof",
                        )),
                        buf,
                    );
                }
                Ok(_) => (),
                Err(e) => {
                    return (Err(e), buf);
                }
            };
        }
    }
}

impl<IO: AsyncReadRent + AsyncWriteRent, C, SD: SideData> AsyncReadRent for Stream<IO, C>
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
        self.read_inner(buf, false)
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

impl<IO: AsyncReadRent + AsyncWriteRent, C, SD: SideData> AsyncWriteRent for Stream<IO, C>
where
    C: DerefMut + Deref<Target = ConnectionCommon<SD>>,
{
    type WriteFuture<'a, T> = impl Future<Output = BufResult<usize, T>>
    where
        T: IoBuf + 'a, Self: 'a;

    type WritevFuture<'a, T> = impl Future<Output = BufResult<usize, T>>
    where
        T: IoVecBuf + 'a, Self: 'a;

    type FlushFuture<'a> = impl Future<Output = io::Result<()>>
    where
        Self: 'a;

    type ShutdownFuture<'a> = impl Future<Output = io::Result<()>>
    where
        Self: 'a;

    fn write<T: IoBuf>(&mut self, buf: T) -> Self::WriteFuture<'_, T> {
        async move {
            // construct slice
            let slice = unsafe { std::slice::from_raw_parts(buf.read_ptr(), buf.bytes_init()) };

            // write slice to rustls
            let n = match self.session.writer().write(slice) {
                Ok(n) => n,
                Err(e) => return (Err(e), buf),
            };

            // write from rustls to connection
            while self.session.wants_write() {
                match self.write_io().await {
                    Ok(0) => {
                        break;
                    }
                    Ok(_) => (),
                    Err(e) => return (Err(e), buf),
                }
            }
            (Ok(n), buf)
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
        async move {
            self.session.writer().flush()?;
            while self.session.wants_write() {
                self.write_io().await?;
            }
            self.io.flush().await
        }
    }

    fn shutdown(&mut self) -> Self::ShutdownFuture<'_> {
        self.session.send_close_notify();
        async move {
            while self.session.wants_write() {
                self.write_io().await?;
            }
            self.io.shutdown().await
        }
    }
}
