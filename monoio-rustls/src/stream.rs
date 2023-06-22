use std::{
    future::Future,
    io::{self, Read, Write},
    ops::{Deref, DerefMut},
};

use monoio::{
    buf::{IoBuf, IoBufMut, IoVecBuf, IoVecBufMut, RawBuf},
    io::{AsyncReadRent, AsyncWriteRent, Split},
    BufResult,
};
use monoio_io_wrapper::{ReadBuffer, WriteBuffer};
use rustls::{ConnectionCommon, ServerConnection, SideData};

#[derive(Debug)]
pub struct Stream<IO, C> {
    pub(crate) io: IO,
    pub(crate) session: C,
    r_buffer: ReadBuffer,
    w_buffer: WriteBuffer,
}

impl<IO> Stream<IO, ServerConnection> {
    #[inline]
    pub fn alpn_protocol(&self) -> Option<&[u8]> {
        self.session.alpn_protocol()
    }
}

unsafe impl<IO: Split, C> Split for Stream<IO, C> {}

impl<IO, C> Stream<IO, C> {
    pub fn new(io: IO, session: C) -> Self {
        Self {
            io,
            session,
            r_buffer: Default::default(),
            w_buffer: Default::default(),
        }
    }

    /// Enable unsafe-io.
    /// # Safety
    /// Users must make sure the buffer ptr and len is valid until io finished.
    /// So the Future cannot be dropped directly. Consider using CancellableIO.
    #[cfg(feature = "unsafe_io")]
    pub unsafe fn new_unsafe(io: IO, session: C) -> Self {
        Self {
            io,
            session,
            r_buffer: ReadBuffer::new_unsafe(),
            w_buffer: WriteBuffer::new_unsafe(),
        }
    }

    pub fn into_parts(self) -> (IO, C) {
        (self.io, self.session)
    }

    pub(crate) fn map_conn<C2, F: FnOnce(C) -> C2>(self, f: F) -> Stream<IO, C2> {
        Stream {
            io: self.io,
            session: f(self.session),
            r_buffer: self.r_buffer,
            w_buffer: self.w_buffer,
        }
    }
}

impl<IO: AsyncReadRent + AsyncWriteRent, C, SD: SideData> Stream<IO, C>
where
    C: DerefMut + Deref<Target = ConnectionCommon<SD>>,
{
    pub(crate) async fn read_io(&mut self, splitted: bool) -> io::Result<usize> {
        let n = loop {
            match self.session.read_tls(&mut self.r_buffer) {
                Ok(n) => {
                    break n;
                }
                Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => {
                    #[allow(unused_unsafe)]
                    unsafe {
                        self.r_buffer.do_io(&mut self.io).await?
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
        let n = loop {
            match self.session.write_tls(&mut self.w_buffer) {
                Ok(n) => {
                    if self.w_buffer.is_safe() {
                        self.w_buffer.do_io(&mut self.io).await?;
                    }
                    break n;
                }
                Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => {
                    // here we don't have to check WouldBlock since we already captured the
                    // mem block info under unsafe-io.
                    #[allow(unused_unsafe)]
                    unsafe {
                        self.w_buffer.do_io(&mut self.io).await?
                    };
                    continue;
                }
                Err(err) => return Err(err),
            }
        };

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

impl<IO: AsyncReadRent + AsyncWriteRent, C, SD: SideData + 'static> AsyncReadRent for Stream<IO, C>
where
    C: DerefMut + Deref<Target = ConnectionCommon<SD>>,
{
    type ReadFuture<'a, T> = impl Future<Output = BufResult<usize, T>> + 'a
    where
        T: IoBufMut + 'a, Self: 'a;

    type ReadvFuture<'a, T> = impl Future<Output = BufResult<usize, T>> + 'a
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

impl<IO: AsyncReadRent + AsyncWriteRent, C, SD: SideData + 'static> AsyncWriteRent for Stream<IO, C>
where
    C: DerefMut + Deref<Target = ConnectionCommon<SD>>,
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
        async move {
            // construct slice
            let slice = unsafe { std::slice::from_raw_parts(buf.read_ptr(), buf.bytes_init()) };

            // flush rustls inner write buffer to make sure there is space for new data
            if self.session.wants_write() {
                if let Err(e) = self.write_io().await {
                    return (Err(e), buf);
                }
            }

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
