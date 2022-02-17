pub mod stream;

use crate::Error::CANAddrIfnameToIndex;
use std::ffi::{c_void, CStr, CString, OsStr};
use std::io::Read;
use std::os::raw::{c_char, c_int, c_short, c_uint};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::io::{AsRawFd, FromRawFd, IntoRawFd, RawFd};
use std::str::FromStr;
use thiserror::Error;

use paste::paste;

pub const CAN_RAW_FD_FRAMES_ENABLE: c_int = 1;

#[derive(Copy, Clone, Eq, PartialEq)]
pub struct Protocol(c_int);

impl Protocol {
    pub const RAW: Protocol = Protocol(libc::CAN_RAW);
    pub const _BCM: Protocol = Protocol(libc::CAN_BCM);
    pub const _TP16: Protocol = Protocol(libc::CAN_TP16);
    pub const _TP20: Protocol = Protocol(libc::CAN_TP20);
    pub const _MCNET: Protocol = Protocol(libc::CAN_MCNET);
    pub const ISOTP: Protocol = Protocol(libc::CAN_ISOTP);
    pub const _J1939: Protocol = Protocol(libc::CAN_J1939);
    pub const _NPROTO: Protocol = Protocol(libc::CAN_NPROTO);
}

#[derive(Copy, Clone, Eq, PartialEq)]
pub struct Type(c_int);

impl Type {
    /// The main kernel CAN driver/UAPI uses RAW sockets for communication
    /// For most normal setups, this type will suffice.
    ///
    /// This applies to CAN2.0 and CANFD, and explicitly **NOT** for CAN-ISOTP
    /// and CAN J1939 (See [`Type::DGRAM`])
    pub const RAW: Type = Type(libc::SOCK_RAW);
    /// DGRAM socket for broadcasting frames[1] and CAN-ISOTP[2] communication
    ///
    /// [1]: https://www.kernel.org/doc/html/latest/networking/can.html#how-to-use-socketcan
    /// [2]: https://github.com/hartkopp/can-isotp/blob/e7597606dfc702484388ea35f9d628a38edd4b69/README.isotp#L88
    pub const DGRAM: Type = Type(libc::SOCK_DGRAM);
}

#[derive(Debug, PartialEq)]
#[non_exhaustive]
pub enum MTU {
    CAN = 16,
    CANFD = 72,
}

impl TryFrom<c_int> for MTU {
    type Error = Error;

    fn try_from(v: c_int) -> Result<Self, Self::Error> {
        match v {
            x if x == MTU::CAN as c_int => Ok(MTU::CAN),
            x if x == MTU::CANFD as c_int => Ok(MTU::CANFD),
            _ => Err(Error::InvalidMTU),
        }
    }
}

type FileDesc = std::os::raw::c_int;

pub struct CANSocket(FileDesc);

impl CANSocket {
    pub fn new(ty: Type, protocol: Protocol) -> std::io::Result<CANSocket> {
        unsafe {
            let fd = libc::socket(libc::PF_CAN, ty.0 | libc::SOCK_CLOEXEC, protocol.0);
            if fd == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(CANSocket(FileDesc::from_raw_fd(fd)))
        }
    }

    fn upgrade(&mut self) -> std::io::Result<()> {
        unsafe {
            let fd = libc::setsockopt(
                self.as_raw_fd(),
                libc::SOL_CAN_RAW,
                libc::CAN_RAW_FD_FRAMES,
                (&CAN_RAW_FD_FRAMES_ENABLE as *const c_int) as *const c_void,
                std::mem::size_of::<c_int>() as u32,
            );
            if fd == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        }
    }

    // TODO: Make nonblocking settings better/more intuitive
    pub fn nonblocking(&self) -> std::io::Result<()> {
        let flags = unsafe { libc::fcntl(self.as_raw_fd(), libc::F_GETFL) };
        if flags < 0 {
            return Err(std::io::Error::last_os_error());
        }

        let ret = unsafe { libc::fcntl(self.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) };
        if ret < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }

    pub fn mtu(&self) -> Result<MTU, Error> {
        let mtu_raw = unsafe { self.mtu_raw() }?;
        mtu_raw.try_into()
    }

    unsafe fn mtu_raw(&self) -> Result<c_int, std::io::Error> {
        let addr: CANAddr = CANAddr::try_from(self.as_raw_fd())?;
        ifreq_siocgifmtu(self.as_raw_fd(), addr.name.as_str())
    }

    pub fn mtu_from_addr(&self, addr: &CANAddr) -> Result<MTU, Error> {
        let mtu_raw = unsafe { self.mtu_raw_from_addr(addr) }?;
        mtu_raw.try_into()
    }

    unsafe fn mtu_raw_from_addr(&self, addr: &CANAddr) -> Result<c_int, std::io::Error> {
        ifreq_siocgifmtu(self.as_raw_fd(), addr.name.as_str())
    }

    /// Binds a CAN socket to the given address
    ///
    /// # Examples
    /// ```no_run
    /// let mut vcan = CANSocket::new(Type::RAW, Protocol::RAW).expect("could not get fd for socket");
    /// let stream = match vcan.bind("vcan0".parse().expect("failed to get ifindex for vcan0")) {
    ///     Ok(stream) => stream,
    ///     Err(e) => {
    ///         println!("failed to bind to vcan0: {:?}", e);
    ///         return
    ///     }
    /// };
    /// ```
    pub fn bind(&mut self, addr: &CANAddr) -> Result<stream::RawStream, Error> {
        let mtu = self.mtu_from_addr(addr)?;
        match mtu {
            MTU::CAN => {}
            MTU::CANFD => self.upgrade()?,
        }

        let bind_ret = unsafe {
            libc::bind(
                self.as_raw_fd() as std::os::raw::c_int,
                (&(addr.inner) as *const CANAddrInner) as *const libc::sockaddr,
                std::mem::size_of::<CANAddrInner>() as c_uint,
            )
        };

        if bind_ret == -1 {
            unsafe {
                libc::close(self.as_raw_fd());
            }
            return Err(std::io::Error::last_os_error().into());
        }

        Ok(stream::RawStream { fd: self.0 })
    }
}

impl AsRawFd for CANSocket {
    fn as_raw_fd(&self) -> RawFd {
        self.0.as_raw_fd()
    }
}

impl IntoRawFd for CANSocket {
    fn into_raw_fd(self) -> RawFd {
        self.0.into_raw_fd()
    }
}

impl FromRawFd for CANSocket {
    unsafe fn from_raw_fd(raw_fd: RawFd) -> Self {
        Self(FromRawFd::from_raw_fd(raw_fd))
    }
}

#[derive(Debug)]
pub struct CANAddr {
    pub name: String,
    inner: CANAddrInner,
}

#[derive(Debug)]
#[repr(C)]
pub(crate) struct CANAddrInner {
    family: c_short,
    ifindex: c_int,
    rx_id: u32,
    tx_id: u32,
}

impl FromStr for CANAddr {
    type Err = crate::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let native_str = OsStr::new(s).as_bytes();
        if native_str.len() > (libc::IF_NAMESIZE - 1) as usize {
            return Err(Error::CANAddrIfnameSize);
        }
        let cstr = CString::new(native_str)?;

        let if_index: c_uint = unsafe { libc::if_nametoindex(cstr.as_ptr()) };
        if if_index == 0 {
            return Err(CANAddrIfnameToIndex {
                source: std::io::Error::last_os_error(),
            });
        }
        Ok(CANAddr {
            name: String::from(s),
            inner: CANAddrInner {
                family: libc::AF_CAN as c_short,
                ifindex: if_index as c_int,
                rx_id: 0,
                tx_id: 0,
            },
        })
    }
}

impl TryFrom<RawFd> for CANAddr {
    type Error = std::io::Error;

    fn try_from(fd: RawFd) -> Result<Self, Self::Error> {
        let inner = CANAddrInner::try_from(fd)?;
        let mut buffer: Vec<c_char> = Vec::with_capacity(libc::IF_NAMESIZE);
        let buffer_ptr = buffer.as_mut_ptr();
        let ret = unsafe { libc::if_indextoname(inner.ifindex as c_uint, buffer_ptr) };
        if ret == std::ptr::null_mut() {
            return Err(std::io::Error::last_os_error());
        }
        let result = unsafe { CStr::from_ptr(buffer_ptr) }
            .to_str()
            .map_err(|_err| std::io::ErrorKind::AddrNotAvailable)?;
        Ok(CANAddr {
            name: String::from(result),
            inner,
        })
    }
}

impl TryFrom<RawFd> for CANAddrInner {
    type Error = std::io::Error;

    fn try_from(fd: RawFd) -> Result<Self, Self::Error> {
        let mut inst = CANAddrInner {
            family: libc::AF_CAN as c_short,
            ifindex: 0,
            rx_id: 0,
            tx_id: 0,
        };

        let ret = unsafe {
            libc::getsockname(
                fd,
                (&mut inst as *mut CANAddrInner) as *mut libc::sockaddr,
                &mut (std::mem::size_of::<CANAddrInner>() as libc::socklen_t),
            )
        };

        if ret < 0 {
            return Err(std::io::Error::last_os_error());
        }

        Ok(inst)
    }
}

/// This stream should be based off of UnixDatagram's `recv_from`
pub struct CANISOTPStream {}

pub trait Frame {
    fn data(&self) -> &[u8];
}

// TODO: Maybe implement a builder pattern? Maybe not.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[repr(C)]
pub struct CANFDFrame {
    pub id: u32,
    pub len: u8,
    pub flags: u8,
    pub res0: u8,
    pub res1: u8,
    pub data: [u8; 64],
}

impl CANFDFrame {
    pub fn new() -> CANFDFrame {
        CANFDFrame {
            id: 0,
            len: 0,
            flags: 0,
            res0: 0,
            res1: 0,
            data: [0u8; 64],
        }
    }
}

impl Frame for CANFDFrame {
    fn data(&self) -> &[u8] {
        &self.data
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[repr(C)]
pub struct CANFrame {
    pub id: u32,
    pub len: u8,
    pub pad0: u8,
    pub pad1: u8,
    pub dlc: u8,
    pub data: [u8; 8],
}

impl CANFrame {
    pub fn new() -> CANFrame {
        CANFrame {
            id: 0,
            len: 0,
            pad0: 0,
            pad1: 0,
            dlc: 0,
            data: [0u8; 8],
        }
    }
}

impl Frame for CANFrame {
    fn data(&self) -> &[u8] {
        &self.data
    }
}

#[derive(Error, Debug)]
pub enum Error {
    #[error("interface name exceeds libc::IF_NAMESIZE")]
    CANAddrIfnameSize,

    #[error("interface name to interface index conversion failed")]
    CANAddrIfnameToIndex { source: std::io::Error },

    #[error("unsupported mtu value that is not CAN2.0 or CANFD")]
    InvalidMTU,

    #[error(transparent)]
    NulError(#[from] std::ffi::NulError),

    #[error(transparent)]
    IOError(#[from] std::io::Error),
}

pub fn try_string_to_ifname<S: AsRef<OsStr> + ?Sized>(
    name: &S,
) -> Result<[u8; libc::IF_NAMESIZE], std::io::Error> {
    let native_str = OsStr::new(name).as_bytes();
    if native_str.len() > (libc::IF_NAMESIZE - 1) as usize {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "ifname too long",
        ));
    }
    let cstr = CString::new(native_str)?;
    let cstr_bytes = cstr.as_bytes_with_nul();
    if cstr_bytes.len() > libc::IF_NAMESIZE {
        // Maybe panic here. This shouldn't _ever_ happen.
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "ifname too long",
        ));
    }

    let mut buf: [u8; libc::IF_NAMESIZE] = [0u8; libc::IF_NAMESIZE];
    buf[..cstr_bytes.len()].clone_from_slice(cstr_bytes);
    return Ok(buf);
}

/// Inline a simple ifreq structure, call it out, then return the value or error
/// TODO: Switch `fd: c_int` parameter to `AsRawFd` trait
macro_rules! ifreq {
    ($name:ident, $req:expr, $reqname:ident, $reqty:ty, $reqdef:expr) => {
        paste! {
            pub(crate) unsafe fn [<ifreq_ $name>]<T: ::std::os::unix::io::AsRawFd, S: AsRef<OsStr> + ?Sized>(fd: T, name: &S) -> Result<$reqty, ::std::io::Error> {
                let name_slice: [u8; ::libc::IF_NAMESIZE] = $crate::try_string_to_ifname(name)?;

                #[repr(C)]
                struct [<_inline_ifreq_ $name>] {
                    ifr_name: [u8; ::libc::IF_NAMESIZE],
                    $reqname: $reqty,
                }

                let mut ifreq: [<_inline_ifreq_ $name>] = [<_inline_ifreq_ $name>] {
                    ifr_name: name_slice,
                    $reqname: $reqdef,
                };

                let ret: ::std::os::raw::c_int = ::libc::ioctl(fd.as_raw_fd(), $req, &mut ifreq);
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
pub const TYPESHIFT: u64 = NRSHIFT + NRBITS as u64;
pub const SIZESHIFT: u64 = TYPESHIFT + TYPEBITS as u64;
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

/// Convert CAN DLC (for both FD and 2.0) into real byte length
///
/// If you were curious on what the most efficient way to do this is
/// (like me) and came up with a match and then a lookup table (also like me),
/// then you'll be pleased (well I was) to read this:
/// - [https://kevinlynagh.com/notes/match-vs-lookup/]
pub fn convert_dlc_to_len(dlc: u8) -> u8 {
    match dlc & 0x0F {
        0..=8 => dlc,
        9 => 12,
        10 => 16,
        11 => 20,
        12 => 24,
        13 => 32,
        14 => 48,
        _ => 64,
    }
}

/// Convert byte length into CAN DLC (for both FD and 2.0)
///
/// See [`crate::convert_dlc_to_len`]'s notes for interesting
/// performance-related information.
pub fn convert_len_to_dlc(len: u8) -> u8 {
    match len {
        0..=8 => len,
        9..=12 => 9,
        13..=16 => 10,
        17..=20 => 11,
        21..=24 => 12,
        25..=32 => 13,
        33..=48 => 14,
        _ => 15,
    }
}
