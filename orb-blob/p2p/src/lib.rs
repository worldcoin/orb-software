//! P2P Blob API

mod hash;
mod tag;

pub use crate::hash::{Hash, HashTopic};
pub use crate::tag::{Tag, TagTopic};

/// Topic for a blob, addressible either by hash or by tag.
#[derive(Debug, Eq, PartialEq, Hash)]
pub enum BlobTopic {
    Hash(HashTopic),
    Tag(TagTopic),
}
