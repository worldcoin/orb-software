//! Definitions for reading and setting inode flags. This is necessary to update efi variables
//! because they are frequently set as `immutable`.

use libc::{c_int, c_long, c_ulong};
use std::{fs::File, io, mem, os::unix::prelude::AsRawFd};

const NRBITS: u32 = 8;
const TYPEBITS: u32 = 8;

const READ: u8 = 2;
const WRITE: u8 = 1;
const SIZEBITS: u8 = 14;

const NRSHIFT: u32 = 0;
const TYPESHIFT: u32 = NRSHIFT + NRBITS;
const SIZESHIFT: u32 = TYPESHIFT + TYPEBITS;
const DIRSHIFT: u32 = SIZESHIFT + SIZEBITS as u32;

/// Lifted from nix and /usr/include/asm-generic/ioctl.h
macro_rules! ioc {
    ($dir:expr, $ty:expr, $nr:expr, $sz:expr) => {
        (($dir as u32) << DIRSHIFT)
            | (($ty as u32) << TYPESHIFT)
            | (($nr as u32) << NRSHIFT)
            | (($sz as u32) << SIZESHIFT)
    };
}

macro_rules! ior {
    ($ty:expr, $nr:expr, $sz:expr) => {
        ioc!(READ, $ty, $nr, $sz)
    };
}

macro_rules! iow {
    ($ty:expr, $nr:expr, $sz:expr) => {
        ioc!(WRITE, $ty, $nr, $sz)
    };
}

#[allow(clippy::cast_possible_truncation)]
const GETFLAGS: c_ulong = ior!(b'f', 1, mem::size_of::<c_long>()) as c_ulong;
#[allow(clippy::cast_possible_truncation)]
const SETFLAGS: c_ulong = iow!(b'f', 2, mem::size_of::<c_long>()) as c_ulong;

pub const IMMUTABLE_MASK: c_int = 0x0000_0010;

/// Gets a file's inode flags.
pub fn read_file_attributes(file: &File) -> io::Result<c_int> {
    let attributes = 0;
    let res = unsafe {
        libc::ioctl(file.as_raw_fd(), GETFLAGS, core::ptr::addr_of!(attributes))
    };
    if res == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(attributes)
}

/// Sets a file's inode flags.
pub fn write_file_attributes(file: &File, attributes: c_int) -> io::Result<()> {
    let res = unsafe {
        libc::ioctl(file.as_raw_fd(), SETFLAGS, core::ptr::addr_of!(attributes))
    };
    if res == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
