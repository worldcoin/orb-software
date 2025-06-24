use alsa_sys::snd_strerror;
use libc::{c_int, c_long};
use std::{ffi::CStr, io};
use thiserror::Error;

/// ALSA result type.
pub type AlsaResult<T> = Result<T, AlsaError>;

/// ALSA error type.
#[allow(clippy::module_name_repetitions)]
#[derive(Error, Debug)]
#[error("{}", .0)]
pub struct AlsaError(pub(crate) &'static str);

impl AsRef<str> for AlsaError {
    fn as_ref(&self) -> &str {
        self.0
    }
}

pub(crate) trait ToAlsaResult: Sized + Copy {
    fn to_alsa_result(self) -> AlsaResult<()> {
        if self.is_negative() {
            Err(AlsaError(
                unsafe { CStr::from_ptr(snd_strerror(self.as_c_int())) }
                    .to_str()
                    .expect("non-UTF-8 error string from alsa"),
            ))
        } else {
            Ok(())
        }
    }

    fn is_negative(self) -> bool;

    fn as_c_int(self) -> c_int;
}

impl ToAlsaResult for c_int {
    fn is_negative(self) -> bool {
        self < 0
    }

    fn as_c_int(self) -> c_int {
        self
    }
}

impl ToAlsaResult for c_long {
    fn is_negative(self) -> bool {
        self < 0
    }

    #[allow(clippy::cast_possible_truncation)]
    fn as_c_int(self) -> c_int {
        self as _
    }
}

pub(crate) fn alsa_to_io_error(err: AlsaError) -> io::Error {
    io::Error::other(err)
}
