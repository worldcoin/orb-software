#![forbid(unsafe_code)]

pub use prost;

pub mod v1 {
    include!(concat!(env!("OUT_DIR"), "/pcp.v1.rs"));
}
