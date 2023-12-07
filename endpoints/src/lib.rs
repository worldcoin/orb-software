//! Logic for selecting backend based on env vars.

// Note that throughout this crate, we don't use thiserror. It would have made the
// code simpler to use it, but I didn't want the dependencies

pub mod backend;
pub mod endpoints;
pub mod orb_id;

pub use crate::backend::Backend;
pub use crate::endpoints::Endpoints;
pub use crate::orb_id::OrbId;
