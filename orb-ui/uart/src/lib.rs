//! UART interface.

#![warn(missing_docs, unsafe_op_in_unsafe_fn)]
#![warn(clippy::pedantic)]
#![allow(clippy::missing_errors_doc)]

mod device;

pub use self::device::BaudRate;
pub use self::device::Device;

use libc::{c_char, c_int, c_ulong, c_void, size_t, ssize_t, termios};
use std::io;

unsafe fn open(path: *const c_char, oflag: c_int) -> io::Result<c_int> {
    let fd = unsafe { libc::open(path, oflag) };
    if fd == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(fd)
    }
}

unsafe fn close(fd: c_int) -> io::Result<()> {
    let result = unsafe { libc::close(fd) };
    if result == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

unsafe fn tcgetattr(fd: c_int, termios: *mut termios) -> io::Result<()> {
    let result = unsafe { libc::tcgetattr(fd, termios) };
    if result == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

unsafe fn tcsetattr(
    fd: c_int,
    optional_actions: c_int,
    termios: *const termios,
) -> io::Result<()> {
    let result = unsafe { libc::tcsetattr(fd, optional_actions, termios) };
    if result == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

unsafe fn ioctl(fd: c_int, request: c_ulong, argp: *mut c_void) -> io::Result<()> {
    let result = unsafe { libc::ioctl(fd, request, argp) };
    if result == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

unsafe fn tcflush(fd: c_int, action: c_int) -> io::Result<()> {
    let result = unsafe { libc::tcflush(fd, action) };
    if result == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

unsafe fn read(fd: c_int, buf: *mut c_void, count: size_t) -> io::Result<ssize_t> {
    let result = unsafe { libc::read(fd, buf, count) };
    if result == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(result)
    }
}

unsafe fn write(fd: c_int, buf: *const c_void, count: size_t) -> io::Result<ssize_t> {
    let result = unsafe { libc::write(fd, buf, count) };
    if result == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(result)
    }
}

unsafe fn tcdrain(fd: c_int) -> io::Result<()> {
    let result = unsafe { libc::tcdrain(fd) };
    if result == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
