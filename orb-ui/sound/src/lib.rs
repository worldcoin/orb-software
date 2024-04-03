//! Orb audio interface.

#![warn(missing_docs, unsafe_op_in_unsafe_fn)]
#![warn(clippy::pedantic)]
#![allow(clippy::missing_errors_doc, clippy::missing_panics_doc)]

mod device;
mod error;
mod hw_params;
mod queue;

pub use self::{
    device::{Device, State},
    error::{AlsaError, AlsaResult},
    hw_params::{Access, Format, HwParams},
    queue::{Queue, SoundBuilder, SoundFuture},
};

use self::error::{alsa_to_io_error, ToAlsaResult};
use libc::{c_int, c_uint, c_void, fd_set, size_t, ssize_t, timeval};
use std::io;

unsafe fn eventfd(init: c_uint, flags: c_int) -> io::Result<c_int> {
    let fd = unsafe { libc::eventfd(init, flags) };
    if fd == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(fd)
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

unsafe fn select(
    nfds: c_int,
    readfs: *mut fd_set,
    writefds: *mut fd_set,
    errorfds: *mut fd_set,
    timeout: *mut timeval,
) -> io::Result<c_int> {
    let result = unsafe { libc::select(nfds, readfs, writefds, errorfds, timeout) };
    if result == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(result)
    }
}
