#![deny(unsafe_op_in_unsafe_fn)]

pub mod addr;
pub mod filter;
pub mod frame;
mod socket;
pub mod stream;

#[cfg(feature = "isotp")]
pub mod isotp;

use std::{
    ffi::{CString, OsStr},
    io::{self, Read},
    os::{
        raw::c_int,
        unix::{
            ffi::OsStrExt,
            io::{AsRawFd, IntoRawFd, RawFd},
        },
    },
};

use addr::{CanAddr, RawCanAddr};
use filter::Filter;
use itertools::Itertools;
use pastey::paste;
use thiserror::Error;

pub use crate::{
    addr::{Protocol, Type},
    frame::{Frame, *},
};

pub const CAN_RAW_FD_FRAMES_ENABLE: c_int = 1;
/// Redefine libc::CAN_RAW_FILTER_MAX to fix crate-specific type constraints
pub const CAN_RAW_FILTER_MAX: usize = 512;

pub const CAN_MTU: usize = 16;
pub const CANFD_MTU: usize = 72;
pub const CAN_DATA_LEN: usize = 8;
pub const CANFD_DATA_LEN: usize = 64;

/// Represents the two possible MTUs (Maximum Transmission Unit) as defined by the CAN standard and
/// the SocketCAN implementation.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
#[repr(u8)]
pub enum MTU {
    /// A classical CAN message is comprised of 8 bytes for the header, 8 bytes for the data.
    CAN = 16,
    /// A flexible datarate / CAN FD message is comprised of the same, backwards compatible 8 bytes
    /// for the header, followed by 64 bytes for the data.
    CANFD = 72,
}

#[cfg(feature = "isotp")]
impl MTU {
    fn to_dlen(mtu: MTU) -> usize {
        match mtu {
            MTU::CAN => CAN_DATA_LEN,
            MTU::CANFD => CANFD_DATA_LEN,
        }
    }

    fn from_dlen(dlen: usize) -> Result<Self, Error> {
        match dlen {
            CAN_DATA_LEN => Ok(MTU::CAN),
            CANFD_DATA_LEN => Ok(MTU::CANFD),
            _ => Err(Error::InvalidDataLength(dlen)),
        }
    }
}

impl TryFrom<c_int> for MTU {
    type Error = Error;

    fn try_from(v: c_int) -> Result<Self, Self::Error> {
        match v {
            x if x == MTU::CAN as c_int => Ok(MTU::CAN),
            x if x == MTU::CANFD as c_int => Ok(MTU::CANFD),
            x => Err(Error::InvalidMtu(x)),
        }
    }
}

impl From<MTU> for u8 {
    fn from(mtu: MTU) -> Self {
        match mtu {
            MTU::CAN => CAN_MTU as u8,
            MTU::CANFD => CANFD_MTU as u8,
        }
    }
}

#[derive(Error, Debug)]
pub enum Error {
    #[error("interface name exceeds libc::IF_NAMESIZE")]
    CanAddrIfnameSize,

    #[error("unable to clone stream")]
    CanStreamClone { source: io::Error },

    #[error("interface name (`{name}`) to interface index failed")]
    CanAddrIfnameToIndex { name: String, source: io::Error },

    #[error("interface index (`{index}`) to interface name failed")]
    CanAddrIfindexToName { index: u32, source: io::Error },

    #[error(
        "parsing result from interface index (`{index}`) to interface name failed"
    )]
    ParseIndexToName {
        index: u32,
        source: std::str::Utf8Error,
    },

    #[error("exceeded `{CAN_RAW_FILTER_MAX}` possible number of filters on socket")]
    CanFilterOverflow(usize),

    #[error("must provide at least one CANFilter")]
    CanFilterMissing,

    // TODO: replace with `intersperse` iterator adapter once `iter_intersparse` is stabilized:
    // https://github.com/rust-lang/rust/issues/79524
    #[error(
        "failed setting filters; filters: [{}]",
        Itertools::intersperse(
            .filters.iter().map(|filter| format!("{filter:?}")),
            ", ".to_string()
        ).collect::<String>()
    )]
    CanFilterError {
        filters: Vec<Filter>,
        source: io::Error,
    },

    #[error("unsupported mtu value that is not CAN2.0 or CANFD")]
    InvalidMtu(i32),

    #[error("invalid frame data length: `{0}`")]
    InvalidDataLength(usize),

    #[error("syscall `{syscall}` failed: `{context:#?}`")]
    Syscall {
        syscall: String,
        context: Option<String>,
        source: io::Error,
    },

    #[error(transparent)]
    NulError(#[from] std::ffi::NulError),

    #[error(transparent)]
    Io(#[from] io::Error),
}

pub fn try_string_to_ifname_bytes<S: AsRef<OsStr> + ?Sized>(
    name: &S,
) -> Result<[u8; libc::IF_NAMESIZE], io::Error> {
    let native_str = OsStr::new(name).as_bytes();
    if native_str.len() > (libc::IF_NAMESIZE - 1) {
        return Err(io::Error::other("ifname too long"));
    }
    let cstr = CString::new(native_str)?;
    let cstr_bytes = cstr.as_bytes_with_nul();
    if cstr_bytes.len() > libc::IF_NAMESIZE {
        // Maybe panic here. This shouldn't _ever_ happen.
        return Err(io::Error::other("ifname too long"));
    }

    let mut buf: [u8; libc::IF_NAMESIZE] = [0u8; libc::IF_NAMESIZE];
    buf[..cstr_bytes.len()].clone_from_slice(cstr_bytes);
    Ok(buf)
}

/// Inline a simple ifreq structure, call it out, then return the value or error
macro_rules! ifreq {
    ($name:ident, $req:expr, $reqname:ident, $reqty:ty, $reqdef:expr) => {
        paste! {
            pub(crate) unsafe fn [<ifreq_ $name>]<T: ::std::os::unix::io::AsRawFd, S: AsRef<OsStr> + ?Sized>(fd: T, name: &S) -> Result<$reqty, ::std::io::Error> {
                let name_slice: [u8; ::libc::IF_NAMESIZE] = $crate::try_string_to_ifname_bytes(name)?;

                #[repr(C)]
                struct [<_inline_ifreq_ $name>] {
                    ifr_name: [u8; ::libc::IF_NAMESIZE],
                    $reqname: $reqty,
                }

                let mut ifreq: [<_inline_ifreq_ $name>] = [<_inline_ifreq_ $name>] {
                    ifr_name: name_slice,
                    $reqname: $reqdef,
                };

                let ret: ::std::os::raw::c_int = unsafe { ::libc::ioctl(fd.as_raw_fd(), $req, &mut ifreq) };
                if ret < 0 {
                    return Err(::std::io::Error::last_os_error())
                }


                Ok(ifreq.$reqname)
            }
        }
    };
}

ifreq!(
    siocgifmtu,
    crate::ioc!(crate::NONE, 137, 33, 0),
    ifr_mtu,
    std::os::raw::c_int,
    0
);

pub const NRBITS: u64 = 8;
pub const TYPEBITS: u64 = 8;

pub const NONE: u8 = 0;
pub const READ: u8 = 2;
pub const WRITE: u8 = 1;
pub const SIZEBITS: u8 = 14;
pub const DIRBITS: u8 = 2;

pub const NRSHIFT: u64 = 0;
pub const TYPESHIFT: u64 = NRSHIFT + NRBITS;
pub const SIZESHIFT: u64 = TYPESHIFT + TYPEBITS;
pub const DIRSHIFT: u64 = SIZESHIFT + SIZEBITS as u64;

pub const NRMASK: u64 = (1 << NRBITS) - 1;
pub const TYPEMASK: u64 = (1 << TYPEBITS) - 1;
pub const SIZEMASK: u64 = (1 << SIZEBITS) - 1;
pub const DIRMASK: u64 = (1 << DIRBITS) - 1;

/// Encode an ioctl command.
#[macro_export]
#[doc(hidden)]
macro_rules! ioc {
    ($dir:expr, $ty:expr, $nr:expr, $sz:expr) => {
        (($dir as u64 & $crate::DIRMASK) << $crate::DIRSHIFT)
            | (($ty as u64 & $crate::TYPEMASK) << $crate::TYPESHIFT)
            | (($nr as u64 & $crate::NRMASK) << $crate::NRSHIFT)
            | (($sz as u64 & $crate::SIZEMASK) << $crate::SIZESHIFT)
    };
}
