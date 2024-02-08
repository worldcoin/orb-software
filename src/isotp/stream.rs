use std::{
    io::{self, Read, Write},
    os::{
        fd::OwnedFd,
        unix::prelude::{AsRawFd, IntoRawFd, RawFd},
    },
};

use super::{
    addr::CanIsotpAddr, flowcontrol::FlowControlOptions, linklayer::LinkLayerOptions, IsotpOptions,
};
use crate::{socket, Error, Protocol, Type, CANFD_DATA_LEN, CAN_DATA_LEN};

pub struct IsotpStream<const N: usize> {
    pub(crate) fd: OwnedFd,
    pub(crate) addr: CanIsotpAddr,
}

pub struct IsotpStreamBuilder<const N: usize> {
    pub(crate) nonblocking: bool,
    pub(crate) isotp_opts: IsotpOptions,
    pub(crate) flow_control_opts: FlowControlOptions,
    pub(crate) link_layer_opts: LinkLayerOptions<N>,
}

impl<const N: usize> IsotpStreamBuilder<N> {
    pub fn new() -> Self {
        Self {
            nonblocking: false,
            isotp_opts: IsotpOptions::default(),
            flow_control_opts: FlowControlOptions::default(),
            link_layer_opts: LinkLayerOptions::<N>::default(),
        }
    }

    pub fn nonblocking(&mut self, nonblocking: bool) -> &mut Self {
        self.nonblocking = nonblocking;
        self
    }

    pub fn isotp_opts(&mut self, opts: IsotpOptions) -> &mut Self {
        self.isotp_opts = opts;
        self
    }

    pub fn flow_control_opts(&mut self, opts: FlowControlOptions) -> &mut Self {
        self.flow_control_opts = opts;
        self
    }

    pub fn link_layer_opts(&mut self, opts: LinkLayerOptions<N>) -> &mut Self {
        self.link_layer_opts = opts;
        self
    }
}

impl IsotpStreamBuilder<CAN_DATA_LEN> {
    pub fn bind(&self, addr: CanIsotpAddr) -> Result<IsotpStream<CAN_DATA_LEN>, Error> {
        imp::bind(
            addr,
            self.nonblocking,
            self.isotp_opts,
            self.flow_control_opts,
            self.link_layer_opts,
        )
    }
}

impl<const N: usize> Default for IsotpStreamBuilder<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl IsotpStreamBuilder<CANFD_DATA_LEN> {
    pub fn bind(&self, addr: CanIsotpAddr) -> Result<IsotpStream<CANFD_DATA_LEN>, Error> {
        imp::bind(
            addr,
            self.nonblocking,
            self.isotp_opts,
            self.flow_control_opts,
            self.link_layer_opts,
        )
    }
}

impl IsotpStream<CAN_DATA_LEN> {
    pub fn build() -> IsotpStreamBuilder<CAN_DATA_LEN> {
        IsotpStreamBuilder::new()
    }

    pub fn new(addr: CanIsotpAddr) -> Result<Self, Error> {
        Ok(Self {
            fd: socket::new(Type::DGRAM, Protocol::ISOTP)?,
            addr,
        })
    }
}

impl IsotpStream<CANFD_DATA_LEN> {
    pub fn build() -> IsotpStreamBuilder<CANFD_DATA_LEN> {
        IsotpStreamBuilder::new()
    }

    pub fn new(addr: CanIsotpAddr) -> Result<Self, Error> {
        Ok(Self {
            fd: socket::new(Type::DGRAM, Protocol::ISOTP)?,
            addr,
        })
    }
}

impl<const N: usize> IsotpStream<N> {
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

impl<const N: usize> Write for IsotpStream<N> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let ret = unsafe {
            libc::write(
                self.as_raw_fd(),
                buf.as_ptr() as *const libc::c_void,
                buf.len(),
            )
        };
        if ret < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(ret as usize)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<const N: usize> Read for IsotpStream<N> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let ret = unsafe {
            libc::read(
                self.as_raw_fd(),
                buf.as_mut_ptr() as *mut libc::c_void,
                buf.len(),
            )
        };
        if ret < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(ret as usize)
    }
}

impl<const N: usize> AsRawFd for IsotpStream<N> {
    fn as_raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }
}

impl<const N: usize> IntoRawFd for IsotpStream<N> {
    fn into_raw_fd(self) -> RawFd {
        self.fd.into_raw_fd()
    }
}

mod imp {
    use std::os::unix::prelude::AsRawFd;

    use super::IsotpStream;
    use crate::{
        isotp::{
            addr::CanIsotpAddr, flowcontrol::FlowControlOptions, linklayer::LinkLayerOptions,
            socket_isotp, IsotpOptions,
        },
        socket, Error, Protocol, Type, CANFD_DATA_LEN, CAN_DATA_LEN,
    };

    pub(crate) trait AllowedToBind {}
    impl AllowedToBind for [(); CAN_DATA_LEN] {}
    impl AllowedToBind for [(); CANFD_DATA_LEN] {}

    pub(super) fn bind<const N: usize>(
        addr: CanIsotpAddr,
        nonblocking: bool,
        isotp_opts: IsotpOptions,
        flow_control_opts: FlowControlOptions,
        link_layer_opts: LinkLayerOptions<N>,
    ) -> Result<IsotpStream<N>, Error>
    where
        [(); N]: AllowedToBind,
    {
        let stream = IsotpStream {
            fd: socket::new(Type::DGRAM, Protocol::ISOTP)?,
            addr,
        };

        bind_fd(
            &stream,
            &stream.addr,
            nonblocking,
            isotp_opts,
            flow_control_opts,
            link_layer_opts,
        )?;

        Ok(stream)
    }

    pub(super) fn bind_fd<const N: usize, T: AsRawFd>(
        fd: &T,
        addr: &CanIsotpAddr,
        nonblocking: bool,
        isotp_opts: IsotpOptions,
        flow_control_opts: FlowControlOptions,
        link_layer_opts: LinkLayerOptions<N>,
    ) -> Result<(), Error>
    where
        [(); N]: AllowedToBind,
    {
        if N == CANFD_DATA_LEN {
            socket::upgrade(fd)?;
        }

        socket::set_nonblocking(fd, nonblocking)?;

        socket_isotp::imp::set_isotp_opts(fd.as_raw_fd(), isotp_opts)?;
        socket_isotp::imp::set_flow_control_opts(fd.as_raw_fd(), flow_control_opts)?;
        socket_isotp::imp::set_link_layer_opts(fd.as_raw_fd(), link_layer_opts)?;

        socket::bind(fd.as_raw_fd(), addr)?;
        Ok(())
    }
}
