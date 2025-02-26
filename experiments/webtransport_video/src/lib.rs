use std::sync::Arc;

use derive_more::{AsRef, Deref, Into};

pub mod control;
pub mod http_server;
pub mod wt_server;

/// Newtype on a vec, to indicate that this contains a png-encoded image.
#[derive(Debug, Into, AsRef, Clone, Deref)]
pub struct EncodedPng(pub Arc<Vec<u8>>);

impl EncodedPng {
    /// Equivalent to [`Self::clone`] but is more explicit that this operation is cheap.
    pub fn clone_cheap(&self) -> Self {
        EncodedPng(Arc::clone(&self.0))
    }
}

impl AsRef<[u8]> for EncodedPng {
    fn as_ref(&self) -> &[u8] {
        self.0.as_slice()
    }
}
