mod newlib;

use libc::ssize_t;
use std::ffi::{CString, OsStr};
use std::os::raw::{c_int, c_short, c_uchar, c_uint, c_ulong, c_void};
use std::os::unix::ffi::OsStrExt;

use anyhow::{anyhow, Result};
/// Things to be done:
/// CAN socket:
/// - Open from interface name (libc FFI)
///   |-> open from interface index
///       |-> build CAN address
///       |-> open socket
///       |-> bind socket
///
/// Best practice: Get familiar with the std::os::unix::io::{FromRawFd, IntoRawFd} traits
/// - Maybe an Inner to hold the FD?
/// - Justify the complicated shenanigans of socket2
///     - If justifiable for normal use, could be good! :smile:
///
/// Ideas for ioctl:
/// - Macro to generate struct and avoid unions?
///
/// Terminology:
/// - EFF -> Extended Frame Format
/// - RTR -> Remote Transmission Request
/// - ERR -> Error Frame
///
/// Good Example:
/// ```text
/// can0  00000080  [12]  08 1A 06 3A 04 08 04 10 20 00 00 00
/// can0  00000080  [12]  08 1A 06 3A 04 08 01 10 19 00 00 00
/// can0  00000080  [12]  08 1A 06 3A 04 08 03 10 18 00 00 00
/// ```
///
/// Useful lings:
/// - CAN frame flags: https://stackoverflow.com/questions/44851066/what-is-the-flags-field-for-in-canfd-frame-in-socketcan
/// -

const AF_NAME: c_uint = 29;
const PF_CAN: c_uint = AF_NAME;

const _CANFD_BRS: c_uchar = 0x01;
const _CANFD_ESI: c_uchar = 0x02;
const _CANFD_FDF: c_uchar = 0x04;

// Golang `iota` </3
const CAN_RAW: c_uint = 1;
const _CAN_BCM: c_uint = 2;
const _CAN_TP16: c_uint = 3;
const _CAN_TP20: c_uint = 4;
const _CAN_MCNET: c_uint = 5;
const _CAN_ISOTP: c_uint = 6;
const _CAN_J1939: c_uint = 7;
const _CAN_NPROTO: c_uint = 8;

const _SOL_CAN_BASE: c_uint = 100;
const _SOL_CAN_RAW: c_uint = _SOL_CAN_BASE + CAN_RAW;

const _CAN_RAW_FILTER: c_uint = 1;
const _CAN_RAW_ERR_FILTER: c_uint = 2;
const _CAN_RAW_LOOPBACK: c_uint = 3;
const _CAN_RAW_RECV_OWN_MSGS: c_uint = 4;
const _CAN_RAW_FD_FRAMES: c_uint = 5;

// Expect only to handle TP binds, no J1939 addressing
#[repr(C)]
pub struct CANAddr {
    family: c_short, // AF_CAN
    ifindex: c_int,
    rx_id: u32,
    tx_id: u32,
}

pub enum MTU {
    Standard = 16, // Size of CAN frame struct
    FD = 72,       // vs. CAN FD frame struct
}

pub struct OpenOptions {
    read: bool,
    write: bool,
    mtu: MTU,
}

impl OpenOptions {
    pub fn new() -> OpenOptions {
        // TODO: Read through https://github.com/linux-can/can-utils/blob/master/candump.c
        // to find sensible READ-ONLY defaults.
        OpenOptions {
            read: false,
            write: false,
            mtu: MTU::Standard,
        }
    }

    pub fn read(&mut self, read: bool) -> &mut Self {
        self.read = read;
        self
    }

    pub fn write(&mut self, write: bool) -> &mut Self {
        self.write = write;
        self
    }

    pub fn mtu(&mut self, mtu: MTU) -> &mut Self {
        self.mtu = mtu;
        self
    }

    pub fn open(&self, ifname: &str) -> Result<CANSocket> {
        CANSocket::open(ifname)
    }
}

pub struct CANSocket {
    name: String,
    fd: c_int,
}
//
// #[repr(C)]
// struct CANSocketIFREQMTU {
//     ifr_name: [u_char; libc::IF_NAMESIZE],
//     ifr_mtu: c_int,
// }

impl CANSocket {
    pub fn open(ifname: &str) -> Result<CANSocket, anyhow::Error> {
        // We need to create a null-terminated string that can be casted to a char*
        let str_bytes = OsStr::new(ifname).as_bytes();
        if str_bytes.len() > (libc::IF_NAMESIZE - 1) as usize {
            return Err(anyhow!("interface name cannot exceed max iface name size"));
        }
        let cstr = CString::new(str_bytes)?; // This gets us the null-terminated string
        println!("converting interface name to index");

        let if_index: c_uint = unsafe { libc::if_nametoindex(cstr.as_ptr()) };
        if if_index == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        println!("translated {} to {}", ifname, if_index);
        CANSocket::open_interface(ifname, if_index)
    }

    pub fn open_interface(ifname: &str, ifindex: c_uint) -> Result<CANSocket, anyhow::Error> {
        let addr = CANAddr {
            family: AF_NAME as c_short,
            ifindex: ifindex as c_int,
            rx_id: 0,
            tx_id: 0,
        };

        println!("opening socket...");
        let sock_fd = unsafe { libc::socket(PF_CAN as c_int, libc::SOCK_RAW, CAN_RAW as c_int) };
        if sock_fd == -1 {
            return Err(std::io::Error::last_os_error().into());
        }

        println!("getting MTU value");
        let mtu_val = unsafe { siocgifmtu(sock_fd, "vcan0") }?;
        println!("received {:?}", mtu_val);

        println!("now binding socket...");

        let bind_ret = unsafe {
            libc::bind(
                sock_fd,
                (&addr as *const CANAddr) as *const libc::sockaddr,
                std::mem::size_of::<CANAddr>() as u32,
            )
        };
        if bind_ret == -1 {
            // We're heading out, so don't bother error checking
            unsafe {
                libc::close(sock_fd);
            }
            return Err(std::io::Error::last_os_error().into());
        }

        Ok(CANSocket {
            name: "".to_string(),
            fd: sock_fd,
        })
    }

    pub fn write_frame(&self, frame: &CANFrame) -> Result<()> {
        const FRAME_SIZE: usize = std::mem::size_of::<CANFrame>();
        let write_ret: ssize_t = unsafe {
            libc::write(
                self.fd,
                (frame as *const CANFrame) as *const c_void,
                std::mem::size_of::<CANFrame>(),
            )
        };
        if write_ret == -1 {
            return Err(std::io::Error::last_os_error().into());
        } else if write_ret != FRAME_SIZE.try_into().unwrap() {
            //TODO: Fix this          ---^
            return Err(anyhow!(
                "expected to write CAN frame struct size ({} bytes) into socket but only wrote {} bytes",
                FRAME_SIZE,
                write_ret
            ));
        }
        Ok(())
    }

    pub fn read_frame(&self) -> Result<CANFrame, anyhow::Error> {
        let mut frame = CANFrame {
            id: 0,
            len: 0,
            pad0: 0,
            pad1: 0,
            dlc: 0,
            data: [0; 8],
        };

        let read_ret = unsafe {
            libc::read(
                self.fd,
                (&mut frame as *mut CANFrame) as *mut c_void,
                std::mem::size_of::<CANFrame>(),
            )
        };
        if read_ret == -1 {
            return Err(std::io::Error::last_os_error().into());
        }
        // TODO: Maybe check on size_of CAN Frame

        Ok(frame)
    }

    pub fn read_fd_frame(&self) -> Result<CANFDFrame, anyhow::Error> {
        const FD_FRAME_SIZE: usize = std::mem::size_of::<CANFDFrame>();
        let mut frame = CANFDFrame {
            id: 0,
            len: 0,
            flags: 0,
            res0: 0,
            res1: 0,
            data: [0; 64],
        };

        let read_ret = unsafe {
            libc::read(
                self.fd,
                (&mut frame as *mut CANFDFrame) as *mut c_void,
                FD_FRAME_SIZE,
            )
        };
        if read_ret == -1 {
            return Err(std::io::Error::last_os_error().into());
        } else if read_ret != FD_FRAME_SIZE.try_into().unwrap() {
            return Err(anyhow!(
                "expected to read {} bytes into CANFDFrame but only read {} bytes",
                FD_FRAME_SIZE,
                read_ret
            ));
        }
        // TODO: Maybe check on size_of CAN Frame
        Ok(frame)
    }

    pub fn write_fd_frame(&self, _frame: &CANFDFrame) {}

    fn close(&mut self) -> Result<()> {
        let ret = unsafe { libc::close(self.fd) };
        if ret != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(())
    }
}

impl Drop for CANSocket {
    fn drop(&mut self) {
        self.close().ok();
    }
}

#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct CANFrame {
    pub id: u32,
    pub len: u8,
    pub pad0: u8,
    pub pad1: u8,
    pub dlc: u8,
    pub data: [u8; 8],
}

// CAN FD frame structure based off kernel net/can.h impl
// See: https://github.com/torvalds/linux/blob/dbe69e43372212527abf48609aba7fc39a6daa27/include/uapi/linux/can.h#L151
// See: https://en.wikipedia.org/wiki/CAN_bus#Extended_frame_format
#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct CANFDFrame {
    // 32 bit CAN_ID + EFF/RTR/SRR bits
    pub id: u32,
    pub len: u8,
    pub flags: u8,
    pub res0: u8,
    pub res1: u8,
    pub data: [u8; 64],
}

// impl CANFDFrame {
//     pub fn new(id: u32, data: &[u8], rtr: bool, err: bool) -> Result<CANFrame, anyhow::Error> {}
// }
//
// #[macro_use]
// macro_rules! ioc {
//     ($dir:expr, $ty:expr, $nr:expr, $sz:expr) => {
//         (($dir as $std::os::raw::c_ulong & $crate::sys::ioctl::DIRMASK) << $crate::DIRSHIFT)
//             | (($ty as $crate::sys::ioctl::ioctl_num_type & $crate::sys::ioctl::TYPEMASK)
//                 << $crate::sys::ioctl::TYPESHIFT)
//             | (($nr as $crate::sys::ioctl::ioctl_num_type & $crate::sys::ioctl::NRMASK)
//                 << $crate::sys::ioctl::NRSHIFT)
//             | (($sz as $crate::sys::ioctl::ioctl_num_type & $crate::sys::ioctl::SIZEMASK)
//                 << $crate::sys::ioctl::SIZESHIFT)
//     };
// }
//
// pub const NRBITS: u64 = 8;
// pub const TYPEBITS: u64 = 8;
// pub const SIZEBITS: u8 = 14;
//
// pub const NRSHIFT: u64 = 0;
// #[doc(hidden)]
// pub const TYPESHIFT: u64 = 8;
// #[doc(hidden)]
// pub const SIZESHIFT: u64 = 16 as u64;
// #[doc(hidden)]
// pub const DIRSHIFT: u64 = 30 as u64;

// 0x8921
// #[derive(Debug)]
// #[repr(C)]
// pub struct tempifreq {
//     ifr_name: [u8; libc::IF_NAMESIZE],
//     ifr_mtu: c_int,
// }

pub unsafe fn siocgifmtu(fd: c_int, name: &str) -> Result<c_int> {
    // let var: std::os::raw::c_long = 0;
    // let dir: i64 = 0;
    // let ty: i64 = 0x89;
    // let nr: i64 = 0x21;
    // let sz: i64 = 0;
    // let req = (dir << )
    // libc::ioctl(fd, ioc!(0 << 32) | 0x89)
    let siocgifmtu_val: c_ulong = 0x890021;
    let newsiocgifmtu_val: c_ulong = ioc!(NONE, 137, 33, 0);

    let str_bytes = OsStr::new(name).as_bytes();
    if str_bytes.len() > libc::IF_NAMESIZE as usize {
        return Err(anyhow!("interface name cannot exceed max iface name size"));
    }
    let cstr = CString::new(str_bytes)?; // This gets us the null-terminated string
    let cstr_bytes = cstr.as_bytes_with_nul();
    if cstr_bytes.len() > libc::IF_NAMESIZE + 1 {
        return Err(anyhow!(
            "cstring interface name cannot exceed max iface name size"
        ));
    }

    #[derive(Debug)]
    #[repr(C)]
    struct tempifreq {
        ifr_name: [u8; libc::IF_NAMESIZE],
        ifr_mtu: c_int,
    }

    let mut ifreq: tempifreq = tempifreq {
        ifr_name: [0u8; libc::IF_NAMESIZE],
        ifr_mtu: 0,
    };

    ifreq.ifr_name[..cstr_bytes.len()].clone_from_slice(cstr_bytes);

    // let ret = libc::ioctl(fd, siocgifmtu_val);
    let ret = libc::ioctl(fd, newsiocgifmtu_val, &mut ifreq);
    if ret == -1 {
        return Err(std::io::Error::last_os_error().into());
    }
    println!("{:?}", ifreq);
    Ok(ret)
}

#[doc(hidden)]
pub const NRBITS: ioctl_num_type = 8;
#[doc(hidden)]
pub const TYPEBITS: ioctl_num_type = 8;

mod consts {
    #[doc(hidden)]
    pub const NONE: u8 = 0;
    #[doc(hidden)]
    pub const READ: u8 = 2;
    #[doc(hidden)]
    pub const WRITE: u8 = 1;
    #[doc(hidden)]
    pub const SIZEBITS: u8 = 14;
    #[doc(hidden)]
    pub const DIRBITS: u8 = 2;
}

pub use self::consts::*;

#[doc(hidden)]
pub const NRSHIFT: ioctl_num_type = 0;
#[doc(hidden)]
pub const TYPESHIFT: ioctl_num_type = NRSHIFT + NRBITS as ioctl_num_type;
#[doc(hidden)]
pub const SIZESHIFT: ioctl_num_type = TYPESHIFT + TYPEBITS as ioctl_num_type;
#[doc(hidden)]
pub const DIRSHIFT: ioctl_num_type = SIZESHIFT + SIZEBITS as ioctl_num_type;

#[doc(hidden)]
pub const NRMASK: ioctl_num_type = (1 << NRBITS) - 1;
#[doc(hidden)]
pub const TYPEMASK: ioctl_num_type = (1 << TYPEBITS) - 1;
#[doc(hidden)]
pub const SIZEMASK: ioctl_num_type = (1 << SIZEBITS) - 1;
#[doc(hidden)]
pub const DIRMASK: ioctl_num_type = (1 << DIRBITS) - 1;

pub type ioctl_num_type = ::std::os::raw::c_ulong;

/// Encode an ioctl command.
#[macro_export]
#[doc(hidden)]
macro_rules! ioc {
    ($dir:expr, $ty:expr, $nr:expr, $sz:expr) => {
        (($dir as $crate::ioctl_num_type & $crate::DIRMASK) << $crate::DIRSHIFT)
            | (($ty as $crate::ioctl_num_type & $crate::TYPEMASK) << $crate::TYPESHIFT)
            | (($nr as $crate::ioctl_num_type & $crate::NRMASK) << $crate::NRSHIFT)
            | (($sz as $crate::ioctl_num_type & $crate::SIZEMASK) << $crate::SIZESHIFT)
    };
}

// ioc!(0x00 0x89 0x21 0x00);
