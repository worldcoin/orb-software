#![forbid(unsafe_code)]
#![expect(clippy::type_complexity)]

#[cfg(feature = "login1")]
pub mod login1;

#[cfg(feature = "systemd1")]
pub mod systemd1;
