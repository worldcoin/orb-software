//! Logic for selecting backend based on env vars.
#![forbid(unsafe_code)]

// Note that throughout this crate, we don't use thiserror. It would have made the
// code simpler to use it, but I didn't want the dependencies

pub mod backend;
pub mod v1;
pub mod v2;

use orb_info::OrbId;

pub use crate::backend::Backend;

/// Safer way to assemble URLs involving `OrbId`
fn concat_urls(prefix: &str, orb_id: &OrbId, suffix: &str) -> url::Url {
    url::Url::parse(prefix)
        .and_then(|url| url.join(&format!("{}/", orb_id.as_str())))
        .and_then(|url| url.join(suffix))
        .expect("urls with validated orb ids should always parse")
}
