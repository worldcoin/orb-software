#![deny(unsafe_op_in_unsafe_fn)]

pub mod camera;
pub mod filters;
pub mod frame;
pub mod frame_format;
pub mod manager;

mod error;

use std::{
    ffi::CStr,
    fmt::Display,
    path::{Path, PathBuf},
    sync::OnceLock,
};

pub use crate::error::ErrorCode;
pub use seek_camera_sys as sys;

#[derive(Debug, Eq, PartialEq, Hash, Clone, Copy)]
#[repr(transparent)]
pub struct SerialNumber(sys::serial_number_t);

impl SerialNumber {
    fn as_str(&self) -> &str {
        // Some platforms have c_char as i8 instead of u8.
        let chars: &[core::ffi::c_char] = &self.0;
        let chars: &[u8] = unsafe { std::mem::transmute(chars) };

        let cs = CStr::from_bytes_until_nul(chars)
            .expect("A null byte should have been present!");
        #[cfg(debug_assertions)]
        return std::str::from_utf8(cs.to_bytes()).expect(
            "Data was not UTF8! We thought this was impossible, post in slack",
        );
        #[cfg(not(debug_assertions))]
        return unsafe { std::str::from_utf8_unchecked(cs.to_bytes()) };
    }
}

impl Display for SerialNumber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Eq, PartialEq, Hash, Clone, Copy)]
#[repr(transparent)]
pub struct ChipId(sys::chipid_t);

impl ChipId {
    fn as_str(&self) -> &str {
        // Some platforms have c_char as i8 instead of u8.
        let chars: &[core::ffi::c_char] = &self.0;
        let chars: &[u8] = unsafe { std::mem::transmute(chars) };

        let cs = CStr::from_bytes_until_nul(chars)
            .expect("A null byte should have been present!");
        #[cfg(debug_assertions)]
        return std::str::from_utf8(cs.to_bytes()).expect(
            "Data was not UTF8! We thought this was impossible, post in slack",
        );
        #[cfg(not(debug_assertions))]
        return unsafe { std::str::from_utf8_unchecked(cs.to_bytes()) };
    }
}

impl Display for ChipId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

pub fn get_seek_dir() -> &'static Path {
    static SEEK_DIR: OnceLock<PathBuf> = OnceLock::new();
    SEEK_DIR.get_or_init(|| {
        let default_seek_dir =
            |_| PathBuf::from(std::env::var("HOME").expect("HOME should be set"));
        #[cfg(windows)]
        let default_seek_dir = |_| {
            PathBuf::from(std::env::var("APPDATA").expect("%APPDATA% should be set"))
        };
        let root = std::env::var("SEEKTHERMAL_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(default_seek_dir);

        #[cfg(unix)]
        return root.join(".seekthermal");
        #[cfg(windows)]
        return root.join("SeekThermal");
    })
}
