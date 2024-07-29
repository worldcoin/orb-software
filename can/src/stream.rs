use std::{io, os::fd::OwnedFd};

use self::imp::{Empty, RawFrame, SetMut};
use crate::{filter::Filter, *};

/// A raw classical and flexible data-rate (FD) compatible CAN frame stream
///
/// After binding the FrameStream to a CAN network interface, individual frames can be [received]
/// and [sent]. If further abstraction is desired, data from the frames can be [read] directly out
/// without working with the [`Frame`] structure in any way.
///
/// The underlying SocketCAN socket and file descriptor will be closed and cleaned when the value
/// is dropped.
///
/// [received]: FrameStream::recv
/// [sent]: FrameStream::send
/// [read]: std::io::Read
/// [`Frame`]: Frame
///
/// # Examples
///
/// ```no_run
/// use can_rs::{
///     addr::CanAddr,
///     stream::FrameStream,
///     Frame,
///     Id,
///     CAN_DATA_LEN,
/// };
///
/// let stream = FrameStream::<CAN_DATA_LEN>::new("can0".parse().unwrap())
///     .expect("failed to bind can0 frame stream");
///
/// stream.send(
///     &Frame {
///         id: Id::Standard(1),
///         flags: 0,
///         len: 8,
///         data: [15u8; 8],
///     },
///     0,
/// );
///
/// let frame = stream.recv_frame(0).unwrap();
/// ```
pub struct FrameStream<const N: usize> {
    pub(crate) fd: OwnedFd,
    pub(crate) addr: CanAddr,
}

/// A builder to configure options on the [`FrameStream`] / socket before binding the underlying
/// socket.
///
/// This improves the ergonomics for initializing a [`FrameStream`] by allowing for method chaining
/// on the various settings.
pub struct FrameStreamBuilder<const N: usize> {
    pub(crate) nonblocking: bool,
    pub(crate) filters: Vec<Filter>,
}

impl<const N: usize> FrameStreamBuilder<N> {
    pub fn new() -> Self {
        Self {
            nonblocking: false,
            filters: vec![],
        }
    }

    pub fn nonblocking(&mut self, nonblocking: bool) -> &mut Self {
        self.nonblocking = nonblocking;
        self
    }

    pub fn filters(&mut self, filters: Vec<Filter>) -> &mut Self {
        self.filters = filters;
        self
    }
}

impl<const N: usize> Default for FrameStreamBuilder<N> {
    fn default() -> Self {
        Self::new()
    }
}

pub trait AllowedToBind: imp::Sealed {}
impl AllowedToBind for [(); CAN_DATA_LEN] {}
impl AllowedToBind for [(); CANFD_DATA_LEN] {}

impl<const N: usize> FrameStreamBuilder<N>
where
    [(); N]: AllowedToBind,
{
    pub fn bind(&self, addr: CanAddr) -> Result<FrameStream<N>, Error> {
        imp::bind(addr, self)
    }
}

impl FrameStream<CAN_DATA_LEN> {
    pub fn new(addr: CanAddr) -> Result<Self, Error> {
        FrameStreamBuilder::<CAN_DATA_LEN>::new().bind(addr)
    }

    pub fn build() -> FrameStreamBuilder<CAN_DATA_LEN> {
        FrameStreamBuilder::new()
    }
}

impl FrameStream<CANFD_DATA_LEN> {
    pub fn new(addr: CanAddr) -> Result<Self, Error> {
        FrameStreamBuilder::<CANFD_DATA_LEN>::new().bind(addr)
    }

    pub fn build() -> FrameStreamBuilder<CANFD_DATA_LEN> {
        FrameStreamBuilder::new()
    }
}

impl<const N: usize> FrameStream<N> {
    pub fn mtu(&self) -> Result<MTU, Error> {
        socket::mtu_from_addr(self, &self.addr)
    }

    pub fn loopback(&self) -> Result<bool, Error> {
        socket::loopback(self)
    }

    pub fn set_filters(&self, filters: &[Filter]) -> Result<(), Error> {
        imp::set_filters_fd(self, filters)
    }

    pub fn filters(&self) -> Result<Vec<Filter>, Error> {
        let ffi_filters = socket::filters(self)?;
        let mut filters = Vec::<Filter>::with_capacity(ffi_filters.len());
        filters.extend(ffi_filters.into_iter().map(Into::into));
        Ok(filters)
    }
}

impl<const N: usize> FrameStream<N> {
    pub fn recv_frame(&self, flags: c_int) -> io::Result<Frame<N>> {
        let mut frame = Frame::empty();
        self.recv(&mut frame, flags).map(|_| frame)
    }

    pub fn recv(&self, frame: &mut Frame<N>, flags: c_int) -> io::Result<usize> {
        let mut raw = RawFrame::empty();
        let size = imp::recv_from(self.as_raw_fd(), &mut raw, flags, Empty)?;
        let _ = std::mem::replace(frame, raw.into());
        Ok(size)
    }

    pub fn recv_from(
        &self,
        frame: &mut Frame<N>,
        flags: c_int,
        src_addr: &mut CanAddr,
    ) -> io::Result<usize> {
        let mut raw = RawFrame::empty();
        let size = imp::recv_from(self.as_raw_fd(), &mut raw, flags, SetMut(src_addr))?;
        let _ = std::mem::replace(frame, raw.into());
        Ok(size)
    }

    pub fn send(&self, frame: &Frame<N>, flags: c_int) -> io::Result<usize> {
        let raw = RawFrame::from(*frame);
        imp::send_to(self, &raw, flags, Empty)
    }

    pub fn send_to(
        &self,
        frame: &Frame<N>,
        flags: c_int,
        dest_addr: &CanAddr,
    ) -> io::Result<usize> {
        let (addr, addr_len): (*const libc::sockaddr, libc::socklen_t) = (
            (&(dest_addr.inner) as *const RawCanAddr) as *const libc::sockaddr,
            std::mem::size_of::<RawCanAddr>() as libc::c_uint,
        );

        unsafe {
            let ret = libc::sendto(
                self.as_raw_fd(),
                (&RawFrame::from(*frame) as *const RawFrame<N>) as *const libc::c_void,
                std::mem::size_of::<RawFrame<N>>(),
                flags,
                addr,
                addr_len,
            );
            if ret < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(ret as usize)
        }
    }

    pub fn try_clone(&self) -> Result<Self, Error> {
        Ok(Self {
            fd: self
                .fd
                .try_clone()
                .map_err(|e| crate::Error::CanStreamClone { source: e })?,
            addr: self.addr.clone(),
        })
    }
}

impl<const N: usize> Read for &FrameStream<N> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut frame: Frame<N> = Frame::empty();
        self.recv(&mut frame, 0)?;
        buf[..(frame.len as usize)]
            .copy_from_slice(&frame.data[..(frame.len as usize)]);
        Ok(frame.len as usize)
    }
}

impl<const N: usize> AsRawFd for FrameStream<N> {
    fn as_raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }
}

impl<const N: usize> IntoRawFd for FrameStream<N> {
    fn into_raw_fd(self) -> RawFd {
        self.fd.into_raw_fd()
    }
}

mod imp {
    use std::{io, os::unix::prelude::AsRawFd};

    use super::{FrameStream, FrameStreamBuilder};
    use crate::{
        addr::CanAddr,
        filter::{Filter, RawFilter},
        socket, Error, Frame, Id, Protocol, RawCanAddr, Type, CANFD_DATA_LEN,
        CAN_DATA_LEN,
    };

    pub trait Sealed {}
    impl Sealed for [(); CAN_DATA_LEN] {}
    impl Sealed for [(); CANFD_DATA_LEN] {}

    pub(super) fn bind<const N: usize>(
        addr: CanAddr,
        options: &FrameStreamBuilder<N>,
    ) -> Result<FrameStream<N>, Error>
    where
        [(); N]: super::AllowedToBind,
    {
        let stream = FrameStream {
            fd: socket::new(Type::RAW, Protocol::RAW)?,
            addr,
        };
        bind_fd(&stream.fd, &stream.addr, options)?;
        Ok(stream)
    }

    pub(super) fn bind_fd<const N: usize, T: AsRawFd>(
        fd: &T,
        addr: &CanAddr,
        options: &FrameStreamBuilder<N>,
    ) -> Result<(), Error>
    where
        [(); N]: super::AllowedToBind,
    {
        if N == CANFD_DATA_LEN {
            socket::upgrade(fd)?;
        }

        socket::set_nonblocking(fd, options.nonblocking)?;

        set_filters_fd(fd, &options.filters)?;
        socket::bind(fd.as_raw_fd(), addr)?;
        Ok(())
    }

    pub(super) fn set_filters_fd<T: AsRawFd>(
        fd: &T,
        filters: &[Filter],
    ) -> Result<(), Error> {
        socket::set_filters(
            fd,
            &filters
                .iter()
                .map(|filter| RawFilter::from(filter.clone()))
                .collect::<Vec<_>>(),
        )
    }

    pub(crate) struct Empty;
    pub(crate) struct Set<T: AsRef<RawCanAddr>>(pub(crate) T);
    pub(crate) struct SetMut<'a, T: AsMut<RawCanAddr>>(pub(crate) &'a mut T);

    pub(crate) trait ToRecvFromArguments {
        fn to_recv_from_arguments(self) -> (*mut libc::sockaddr, *mut libc::socklen_t);
    }

    impl ToRecvFromArguments for Empty {
        fn to_recv_from_arguments(self) -> (*mut libc::sockaddr, *mut libc::socklen_t) {
            (std::ptr::null_mut(), std::ptr::null_mut())
        }
    }

    impl<T: AsMut<RawCanAddr>> ToRecvFromArguments for SetMut<'_, T> {
        fn to_recv_from_arguments(self) -> (*mut libc::sockaddr, *mut libc::socklen_t) {
            (
                self.0.as_mut() as *mut RawCanAddr as *mut libc::sockaddr,
                std::mem::size_of::<RawCanAddr>() as libc::c_uint
                    as *mut libc::socklen_t,
            )
        }
    }

    pub(crate) trait ToSendToArguments {
        fn to_send_to_arguments(self) -> (*const libc::sockaddr, libc::socklen_t);
    }

    impl ToSendToArguments for Empty {
        fn to_send_to_arguments(self) -> (*const libc::sockaddr, libc::socklen_t) {
            (std::ptr::null_mut(), 0)
        }
    }

    impl<T: AsRef<RawCanAddr>> ToSendToArguments for Set<T> {
        fn to_send_to_arguments(self) -> (*const libc::sockaddr, libc::socklen_t) {
            (
                self.0.as_ref() as *const RawCanAddr as *const libc::sockaddr,
                std::mem::size_of::<RawCanAddr>() as libc::c_uint,
            )
        }
    }

    impl<const N: usize> From<RawFrame<N>> for Frame<N> {
        fn from(raw: RawFrame<N>) -> Self {
            Self {
                id: Id::from(raw.id),
                len: raw.len,
                flags: raw.flags,
                data: raw.data,
            }
        }
    }

    #[repr(C)]
    pub(super) struct RawFrame<const N: usize> {
        id: u32,
        len: u8,
        flags: u8,
        res0: u8,
        res1: u8,
        data: [u8; N],
    }

    impl<const N: usize> RawFrame<N> {
        pub(crate) fn empty() -> Self {
            Self {
                id: 0,
                len: 0,
                flags: 0,
                res0: 0,
                res1: 0,
                data: [0u8; N],
            }
        }
    }

    impl<const N: usize> From<Frame<N>> for RawFrame<N> {
        fn from(frame: Frame<N>) -> Self {
            Self {
                id: frame.id.wire_value(),
                len: frame.len,
                flags: frame.flags,
                res0: 0,
                res1: 0,
                data: frame.data,
            }
        }
    }

    pub(super) fn send_to<const N: usize, F, T>(
        fd: &F,
        frame: &RawFrame<N>,
        flags: libc::c_int,
        dest_addr: T,
    ) -> io::Result<usize>
    where
        F: AsRawFd,
        T: ToSendToArguments,
    {
        let (addr, addr_len) = dest_addr.to_send_to_arguments();
        let ret = unsafe {
            libc::sendto(
                fd.as_raw_fd(),
                (frame as *const RawFrame<N>) as *const libc::c_void,
                std::mem::size_of::<RawFrame<N>>(),
                flags,
                addr,
                addr_len,
            )
        };

        if ret < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(ret as usize)
    }

    pub(super) fn recv_from<
        const N: usize,
        F: AsRawFd + std::fmt::Debug,
        T: ToRecvFromArguments,
    >(
        fd: F,
        frame: &mut RawFrame<N>,
        flags: libc::c_int,
        src_addr: T,
    ) -> io::Result<usize> {
        let (addr, addrlen) = src_addr.to_recv_from_arguments();
        let ret = unsafe {
            libc::recvfrom(
                fd.as_raw_fd(),
                (frame as *mut RawFrame<N>) as *mut libc::c_void,
                std::mem::size_of::<RawFrame<N>>(),
                flags,
                addr,
                addrlen,
            )
        };
        if ret < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(ret as usize)
    }
}
