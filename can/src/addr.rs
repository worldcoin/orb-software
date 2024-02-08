use std::{
    ffi::{CStr, CString, OsStr},
    io,
    os::unix::prelude::{OsStrExt, RawFd},
    str::FromStr,
};

use crate::Error;

/// Redefined AF_CAN from libc::AF_CAN with the correct unsigned type
pub const AF_CAN: u16 = 29;

#[derive(Copy, Clone, Eq, PartialEq)]
pub struct Protocol(pub(crate) libc::c_int);

impl Protocol {
    pub const ISOTP: Protocol = Protocol(libc::CAN_ISOTP);
    pub const RAW: Protocol = Protocol(libc::CAN_RAW);
    pub const _BCM: Protocol = Protocol(libc::CAN_BCM);
    pub const _J1939: Protocol = Protocol(libc::CAN_J1939);
    pub const _MCNET: Protocol = Protocol(libc::CAN_MCNET);
    pub const _NPROTO: Protocol = Protocol(libc::CAN_NPROTO);
    pub const _TP16: Protocol = Protocol(libc::CAN_TP16);
    pub const _TP20: Protocol = Protocol(libc::CAN_TP20);
}

#[derive(Copy, Clone, Eq, PartialEq)]
pub struct Type(pub(crate) libc::c_int);

impl Type {
    /// DGRAM socket for broadcasting frames[1] and CAN-ISOTP[2] communication
    ///
    /// [1]: https://www.kernel.org/doc/html/latest/networking/can.html#how-to-use-socketcan
    /// [2]: https://github.com/hartkopp/can-isotp/blob/e7597606dfc702484388ea35f9d628a38edd4b69/README.isotp#L88
    pub const DGRAM: Type = Type(libc::SOCK_DGRAM);
    /// The main kernel CAN driver/UAPI uses RAW sockets for communication
    /// For most normal setups, this type will suffice.
    ///
    /// This applies to CAN2.0 and CANFD, and explicitly **NOT** for CAN-ISOTP
    /// and CAN J1939 (See [`Type::DGRAM`])
    pub const RAW: Type = Type(libc::SOCK_RAW);
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct CanAddr {
    pub name: String,
    pub(crate) inner: RawCanAddr,
}

impl CanAddr {
    pub fn new(name: &str) -> Result<Self, Error> {
        Self::from_str(name)
    }
}

impl From<CanAddr> for RawCanAddr {
    fn from(addr: CanAddr) -> Self {
        addr.inner
    }
}

impl AsMut<RawCanAddr> for CanAddr {
    fn as_mut(&mut self) -> &mut RawCanAddr {
        &mut self.inner
    }
}

impl AsRef<RawCanAddr> for CanAddr {
    fn as_ref(&self) -> &RawCanAddr {
        &self.inner
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
#[repr(C)]
pub struct RawCanAddr {
    pub(crate) family: u16,
    pub(crate) ifindex: i32,
    pub(crate) rx_id: u32,
    pub(crate) tx_id: u32,
}

impl FromStr for CanAddr {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let if_index = try_string_to_ifindex(s)?;
        Ok(Self {
            name: String::from(s),
            inner: RawCanAddr {
                family: AF_CAN,
                ifindex: if_index as libc::c_int,
                rx_id: 0,
                tx_id: 0,
            },
        })
    }
}

impl TryFrom<RawFd> for CanAddr {
    type Error = Error;

    fn try_from(fd: RawFd) -> Result<Self, Error> {
        let inner = RawCanAddr::try_from(fd)?;

        let mut buffer: Vec<libc::c_char> = Vec::with_capacity(libc::IF_NAMESIZE);
        let buffer_ptr = buffer.as_mut_ptr();
        let ret =
            unsafe { libc::if_indextoname(inner.ifindex as libc::c_uint, buffer_ptr) };
        if ret.is_null() {
            return Err(Error::CanAddrIfindexToName {
                index: inner.ifindex as u32,
                source: io::Error::last_os_error(),
            });
        }
        let result = unsafe { CStr::from_ptr(buffer_ptr) }
            .to_str()
            .map_err(|err| Error::ParseIndexToName {
                index: inner.ifindex as u32,
                source: err,
            })?;
        Ok(Self {
            name: String::from(result),
            inner,
        })
    }
}

impl TryFrom<RawFd> for RawCanAddr {
    type Error = Error;

    fn try_from(fd: RawFd) -> Result<Self, Error> {
        let mut inst = Self {
            family: AF_CAN,
            ifindex: 0,
            rx_id: 0,
            tx_id: 0,
        };

        let mut len = std::mem::size_of::<RawCanAddr>();

        let ret = unsafe {
            libc::getsockname(
                fd,
                (&mut inst as *mut RawCanAddr) as *mut libc::sockaddr,
                &mut len as *mut usize as *mut libc::socklen_t,
            )
        };

        if ret < 0 {
            return Err(Error::Syscall {
                syscall: "getsockname(2)".to_string(),
                context: Some("getting bound sockaddr from fd".to_string()),
                source: io::Error::last_os_error(),
            });
        }

        Ok(inst)
    }
}

pub fn try_ifindex_to_ifname<T: AsRef<RawCanAddr>>(addr: T) -> Result<String, Error> {
    let mut buffer: Vec<libc::c_char> = Vec::with_capacity(libc::IF_NAMESIZE);
    let buffer_ptr = buffer.as_mut_ptr();
    let ret = unsafe {
        libc::if_indextoname(addr.as_ref().ifindex as libc::c_uint, buffer_ptr)
    };
    if ret.is_null() {
        return Err(Error::CanAddrIfindexToName {
            index: addr.as_ref().ifindex as u32,
            source: io::Error::last_os_error(),
        });
    }
    unsafe { CStr::from_ptr(buffer_ptr) }
        .to_str()
        .map_err(|_err| Error::CanAddrIfindexToName {
            index: addr.as_ref().ifindex as u32,
            source: io::Error::new(
                io::ErrorKind::InvalidData,
                "building address from if_indextoname glibc function return failed",
            ),
        })
        .map(|s| s.to_owned())
}

pub fn try_string_to_ifindex<S: AsRef<OsStr> + ?Sized>(
    name: &S,
) -> Result<libc::c_uint, Error> {
    let if_name = try_string_to_ifname(name)?;
    let if_index: libc::c_uint = unsafe { libc::if_nametoindex(if_name.as_ptr()) };
    if if_index == 0 {
        return Err(Error::CanAddrIfnameToIndex {
            name: name.as_ref().to_string_lossy().to_string(),
            source: io::Error::last_os_error(),
        });
    }
    Ok(if_index)
}

pub fn try_string_to_ifname<S: AsRef<OsStr> + ?Sized>(
    name: &S,
) -> Result<CString, Error> {
    let native_str = OsStr::new(name).as_bytes();
    if native_str.len() > (libc::IF_NAMESIZE - 1) {
        return Err(Error::CanAddrIfnameToIndex {
            name: name.as_ref().to_string_lossy().to_string(),
            source: io::Error::last_os_error(),
        });
    }
    let cstr = CString::new(native_str)?;
    if cstr.as_bytes_with_nul().len() > libc::IF_NAMESIZE {
        // Maybe panic here. This shouldn't _ever_ happen.
        return Err(Error::CanAddrIfnameToIndex {
            name: name.as_ref().to_string_lossy().to_string(),
            source: io::Error::last_os_error(),
        });
    }
    Ok(cstr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_interface_address() -> Result<(), Error> {
        let addr: CanAddr = "vcan0".parse()?;
        assert_eq!("vcan0", addr.name);
        Ok(())
    }
}
