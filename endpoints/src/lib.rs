//! Logic for selecting backend based on env vars.
#![forbid(unsafe_code)]

// Note that throughout this crate, we don't use thiserror. It would have made the
// code simpler to use it, but I didn't want the dependencies

pub mod backend;
pub mod orb_id;
pub mod v1;
pub mod v2;

// Backwards compat
pub use crate::v1::endpoints;

pub use crate::backend::Backend;
pub use crate::orb_id::OrbId;
pub use crate::v1::endpoints::Endpoints;
