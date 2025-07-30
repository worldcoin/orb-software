//! The hashes api

#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Hash(iroh_blobs::Hash);

/// Topic for a blob's hash
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct HashTopic {
    pub hash: Hash,
}
