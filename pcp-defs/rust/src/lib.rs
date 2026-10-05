#![forbid(unsafe_code)]

pub use prost;

// pbjson-generated serde impls trip this lint; the code is not ours to edit.
#[allow(clippy::useless_borrows_in_formatting)]
pub mod v1 {
    include!(concat!(env!("OUT_DIR"), "/pcp.v1.rs"));
    include!(concat!(env!("OUT_DIR"), "/pcp.v1.serde.rs"));
}
