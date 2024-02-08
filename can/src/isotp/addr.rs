use std::{ffi::CStr, io, os::unix::prelude::RawFd};

use crate::{
    addr::{try_string_to_ifindex, RawCanAddr, AF_CAN},
    Error, Id,
};

#[derive(Debug, Clone)]
pub struct CanIsotpAddr {
    pub name: String,
    pub rx_id: Id,
    pub tx_id: Id,
    pub(crate) inner: RawCanAddr,
}

impl CanIsotpAddr {
    pub fn new(name: &str, tx_id: Id, rx_id: Id) -> Result<Self, Error> {
        let if_index = try_string_to_ifindex(name)?;
        Ok(CanIsotpAddr {
            name: String::from(name),
            tx_id,
            rx_id,
            inner: RawCanAddr {
                family: AF_CAN,
                ifindex: if_index as libc::c_int,
                tx_id: tx_id.wire_value(),
                rx_id: rx_id.wire_value(),
            },
        })
    }
}

impl From<CanIsotpAddr> for RawCanAddr {
    fn from(addr: CanIsotpAddr) -> Self {
        addr.inner
    }
}

impl AsMut<RawCanAddr> for CanIsotpAddr {
    fn as_mut(&mut self) -> &mut RawCanAddr {
        &mut self.inner
    }
}

impl AsRef<RawCanAddr> for CanIsotpAddr {
    fn as_ref(&self) -> &RawCanAddr {
        &self.inner
    }
}

impl TryFrom<RawFd> for CanIsotpAddr {
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
        Ok(CanIsotpAddr {
            name: String::from(result),
            tx_id: Id::from(inner.tx_id),
            rx_id: Id::from(inner.rx_id),
            inner,
        })
    }
}
