use serde::{Deserialize, Serialize};

pub mod connd;
pub mod core;

pub use connd::*;
pub use core::*;
use zenorb::zenoh::bytes::OptionZBytes;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub enum Mode {
    /// Forwards the message immedately through the OES
    Normal,
    /// Forwards the message immedately through the OES but also stores it in the cache.
    /// The last cached message of each event is always sent to the backend every 30s.
    Sticky,
    /// Forwards the message strictly to the OES cache.
    /// The last cached message of each event is always sent to the backend every 30s.
    CacheOnly,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct Headers {
    pub mode: Mode,
}

impl Default for Headers {
    fn default() -> Self {
        Self { mode: Mode::Normal }
    }
}

impl Headers {
    pub fn mode(mut self, mode: Mode) -> Self {
        self.mode = mode;
        self
    }
}

impl From<Headers> for OptionZBytes {
    fn from(val: Headers) -> Self {
        serde_json::to_vec(&val).ok().into()
    }
}
