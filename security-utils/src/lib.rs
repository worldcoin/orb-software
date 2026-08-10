#![forbid(unsafe_code)]

pub mod certs;

#[cfg(feature = "reqwest")]
pub mod reqwest;
