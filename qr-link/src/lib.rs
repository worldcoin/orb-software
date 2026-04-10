//! Data link between Worldcoin App and Orb through QR-codes.
//!
//! Worldcoin App needs to transfer considerable amount of data to an Orb to
//! begin a new signup. Encoding all the data into a single or a series of QR-
//! codes would compromise QR scanning performance. On the other hand just
//! letting the Orb to download all the data from the backend would compromise
//! security.
//!
//! Therefore we employ a hybrid approach, where we transfer the data via the
//! backend for performance, and transfer a hash of the data via a QR-code for
//! security.
//!
//! This crate handles QR-code encoding and decoding. Hashing and verification
//! of `AppAuthenticatedData` lives in the `orb-relay-messages` crate.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

#[cfg(feature = "decode")]
mod decode;
#[cfg(feature = "encode")]
mod encode;
#[cfg(feature = "decode")]
pub use decode::{decode_and_verify_qr, DecodeError};
#[cfg(feature = "encode")]
pub use encode::{encode_static_qr, encode_static_qr_v5};
#[cfg(feature = "decode")]
pub use orb_relay_messages::common::v1::AppAuthenticatedData;

pub(crate) const QR_VERSION_4: u8 = 4;
pub(crate) const QR_VERSION_5: u8 = 5;
