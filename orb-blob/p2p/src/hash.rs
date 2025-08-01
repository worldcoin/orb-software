//! The hashes api

use iroh::NodeId;
use serde::{Deserialize, Serialize};

pub use iroh_blobs::Hash;

use crate::BlobRef;

// TODO: Use something more space efficient and evolvable than json
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct HashGossipMsg {
    pub blob_ref: BlobRef,
    pub node_id: NodeId,
}
