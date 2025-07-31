//! The hashes api

use iroh::NodeId;
use iroh_gossip::proto::TopicId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::HASH_CTX;

#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Hash(pub iroh_blobs::Hash);

/// Topic for a blob's hash
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct HashTopic {
    pub hash: Hash,
}

impl HashTopic {
    pub(crate) fn to_id(&self) -> TopicId {
        let mut hasher: Sha256 = sha2::Digest::new();
        hasher.update(HASH_CTX);
        hasher.update("hash");
        hasher.update(self.hash.0.as_bytes());
        let hash: [u8; 32] = hasher.finalize().into();

        TopicId::from_bytes(hash)
    }
}

// TODO: Use something more space efficient and evolvable than json
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct HashGossipMsg {
    pub node_id: NodeId,
}
