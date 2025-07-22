use std::{
    io,
    os::{
        fd::OwnedFd,
        unix::{io::FromRawFd, prelude::AsRawFd},
    },
};

use libc::CAN_RAW_LOOPBACK;

use crate::{
    addr::{try_ifindex_to_ifname, RawCanAddr},
    filter::{Filter, RawFilter},
    ifreq_siocgifmtu, Error, Protocol, Type, CAN_RAW_FD_FRAMES_ENABLE,
    CAN_RAW_FILTER_MAX, MTU,
};

pub(crate) fn new(ty: Type, protocol: Protocol) -> Result<OwnedFd, Error> {
    unsafe {
        let fd = libc::socket(libc::PF_CAN, ty.0 | libc::SOCK_CLOEXEC, protocol.0);
        if fd == -1 {
            return Err(Error::Syscall {
                syscall: "socket(2)".to_string(),
                context: None,
                source: io::Error::last_os_error(),
            });
        }
        Ok(OwnedFd::from_raw_fd(fd))
    }
}

pub(crate) fn upgrade<T: AsRawFd>(fd: &T) -> Result<(), Error> {
    let ret = unsafe {
        libc::setsockopt(
            fd.as_raw_fd(),
            libc::SOL_CAN_RAW,
            libc::CAN_RAW_FD_FRAMES,
            (&CAN_RAW_FD_FRAMES_ENABLE as *const libc::c_int) as *const libc::c_void,
            std::mem::size_of::<libc::c_int>() as u32,
        )
    };
    if ret < 0 {
        return Err(Error::Syscall {
            syscall: "setsockopt(2)".to_string(),
            context: Some("setting CAN_RAW_FD_FRAMES_ENABLE".to_string()),
            source: io::Error::last_os_error(),
        });
    }
    Ok(())
}

pub(crate) fn loopback<T: AsRawFd>(fd: &T) -> Result<bool, Error> {
    let mut loopback = false;
    let mut loopback_buf_size = std::mem::size_of::<bool>() as u32;
    let ret = unsafe {
        libc::getsockopt(
            fd.as_raw_fd(),
            libc::SOL_CAN_RAW,
            CAN_RAW_LOOPBACK,
            std::ptr::addr_of_mut!(loopback).cast::<libc::c_void>(),
            std::ptr::addr_of_mut!(loopback_buf_size),
        )
    };
    if ret < 0 {
        return Err(Error::Syscall {
            syscall: "getsockopt(2)".to_string(),
            context: Some("getting CAN_RAW_LOOPBACK".to_string()),
            source: io::Error::last_os_error(),
        });
    }
    Ok(loopback)
}

/// Set socket to nonblocking mode
///
/// Subsequent `read` or `recv` calls on the bound socket may result in a `EAGAIN` or
/// `EWOULDBLOCK` error if the call would have been blocking. For details, see
/// <https://man7.org/linux/man-pages/man7/socket.7.html>.
pub(crate) fn set_nonblocking<T: AsRawFd>(
    fd: &T,
    nonblocking: bool,
) -> Result<(), Error> {
    let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFL) };
    if flags < 0 {
        return Err(Error::Syscall {
            syscall: "fcntl(2)".to_string(),
            context: Some("getting socket flags".to_string()),
            source: io::Error::last_os_error(),
        });
    }

    let new_flags = if nonblocking {
        flags | libc::O_NONBLOCK
    } else {
        flags & !libc::O_NONBLOCK
    };

    if flags != new_flags {
        let ret = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, new_flags) };
        if ret < 0 {
            return Err(Error::Syscall {
                syscall: "fcntl(2)".to_string(),
                context: Some("setting socket O_NONBLOCK flag".to_string()),
                source: io::Error::last_os_error(),
            });
        }
    }
    Ok(())
}

pub(crate) fn mtu_from_addr<T: AsRawFd, R: AsRef<RawCanAddr>>(
    fd: &T,
    addr: R,
) -> Result<MTU, Error> {
    let mtu_raw = unsafe { mtu_raw_from_addr(fd, addr) }?;
    mtu_raw.try_into()
}

unsafe fn mtu_raw_from_addr<T: AsRawFd, R: AsRef<RawCanAddr>>(
    fd: &T,
    addr: R,
) -> Result<libc::c_int, Error> {
    unsafe { ifreq_siocgifmtu(fd.as_raw_fd(), &try_ifindex_to_ifname(addr)?) }.map_err(
        |err| Error::Syscall {
            syscall: "ioctl(2)".to_string(),
            context: Some(
                "ifreq (netdevice(7)) to get socket MTU from sockaddr_can".to_string(),
            ),
            source: err,
        },
    )
}

/// Binds a CAN socket to the given address
///
/// # Examples
/// ```no_compile
/// use update_agent_can::{Protocol, Type};
/// let mut vcan = socket::new(Type::RAW, Protocol::RAW).expect("could not get fd for socket");
/// let stream = match vcan.bind("vcan0".parse().expect("failed to get ifindex for vcan0")) {
///     Ok(stream) => stream,
///     Err(e) => {
///         println!("failed to bind to vcan0: {:?}", e);
///         return;
///     }
/// };
/// ```
pub(crate) fn bind<T: AsRawFd, R: AsRef<RawCanAddr>>(
    fd: T,
    addr: R,
) -> Result<(), Error> {
    let bind_ret = unsafe {
        libc::bind(
            fd.as_raw_fd() as std::os::raw::c_int,
            (addr.as_ref() as *const RawCanAddr) as *const libc::sockaddr,
            std::mem::size_of::<RawCanAddr>() as libc::c_uint,
        )
    };

    if bind_ret == -1 {
        unsafe {
            libc::close(fd.as_raw_fd());
        }
        return Err(Error::Syscall {
            syscall: "bind(2)".to_string(),
            context: None,
            source: io::Error::last_os_error(),
        });
    }
    Ok(())
}

/// Filters messages such that the socket only receives frames whose SFF/EFF ID
/// bitwise AND a CAN filter mask matches the bitwise AND of that same CAN filter's
/// mask.
///
/// > CANFrame::id & CANFilter::mask == CANFilter::id & CANFilter::mask
///
/// Note: This resets any previously set filters
pub(crate) fn set_filters<T: AsRawFd>(
    fd: &T,
    filters: &[RawFilter],
) -> Result<(), Error> {
    if filters.len() > CAN_RAW_FILTER_MAX {
        return Err(Error::CanFilterOverflow(filters.len()));
    }
    let ret = unsafe {
        libc::setsockopt(
            fd.as_raw_fd(),
            libc::SOL_CAN_RAW,
            libc::CAN_RAW_FILTER,
            filters.as_ptr() as *const libc::c_void,
            std::mem::size_of_val(filters) as u32,
        )
    };
    if ret < 0 {
        return Err(Error::CanFilterError {
            filters: filters
                .iter()
                .map(|filter| Filter::from(filter.clone()))
                .collect::<Vec<Filter>>(),
            source: io::Error::last_os_error(),
        });
    }
    Ok(())
}

/// Read filters associated with CAN socket
///
/// Will allocate exponentially increasing buffer to accommodate huge numbers of filters.
/// Note: This will use one extra syscall if the socket has N%2==0 filters
pub(crate) fn filters<T: AsRawFd>(fd: &T) -> Result<Vec<RawFilter>, Error> {
    let mut buf = vec![RawFilter::empty(); 4];
    loop {
        // We re-read all of the filters 0..N with each loop pass
        let n_filters_read = filters_raw(fd, buf.as_mut_slice());
        match n_filters_read {
            Ok(n_elem)
                if n_elem < buf.len()
                    || (n_elem == CAN_RAW_FILTER_MAX
                        && buf.len() == CAN_RAW_FILTER_MAX) =>
            {
                buf.truncate(n_elem);
                break;
            }
            Ok(_) => {
                // If the buffer isn't large enough, filters_raw (kernel < 5.12) won't return an
                // error but fill the available buffer and return the number of filters read
                // see https://elixir.bootlin.com/linux/v5.10/source/net/can/raw.c#L663
                // this is fixed in kernel 5.12 https://elixir.bootlin.com/linux/v5.12/source/net/can/raw.c#L665
                // let's try to grow it exponentially as long as it doesn't
                // exceed CAN_RAW_FILTER_MAX then retry
                let expansion = std::cmp::min(buf.len() * 2, crate::CAN_RAW_FILTER_MAX);
                buf.resize_with(expansion, RawFilter::empty);
            }
            Err(Error::Syscall { source, .. })
                if source.raw_os_error() == Some(libc::ERANGE)
                    && buf.len() < crate::CAN_RAW_FILTER_MAX =>
            {
                // If the buffer isn't large enough, grow it exponentially as long as it doesn't
                // exceed CAN_RAW_FILTER_MAX then retry
                let expansion = std::cmp::min(buf.len() * 2, crate::CAN_RAW_FILTER_MAX);
                buf.resize_with(expansion, RawFilter::empty);
            }
            Err(e) => {
                return Err(e);
            }
        };
    }
    Ok(buf)
}

/// Read N filters directly into buffer of size N and return the number of filters read
pub(crate) fn filters_raw<T: AsRawFd>(
    fd: &T,
    buf: &mut [RawFilter],
) -> Result<usize, Error> {
    let mut len = std::mem::size_of_val(buf) as u32;
    let ret = unsafe {
        libc::getsockopt(
            fd.as_raw_fd(),
            libc::SOL_CAN_RAW,
            libc::CAN_RAW_FILTER,
            buf.as_mut_ptr() as *mut libc::c_void,
            &mut len,
        )
    };
    if ret < 0 {
        return Err(Error::Syscall {
            syscall: "getsockopt(2)".to_string(),
            context: Some("getting CAN_RAW_FILTER".to_string()),
            source: io::Error::last_os_error(),
        });
    }
    let len = len as usize;
    if len % std::mem::size_of::<RawFilter>() != 0 {
        return Err(Error::CanFilterError {
            filters: buf
                .iter()
                .map(|inner| From::from(inner.clone()))
                .collect::<Vec<_>>(),
            source: io::Error::other(format!(
                "bad read state when reading filters: read `{len}` bytes which is not a \
                     multiple of {}",
                std::mem::size_of::<RawFilter>(),
            )),
        });
    }
    Ok(len / std::mem::size_of::<RawFilter>())
}
