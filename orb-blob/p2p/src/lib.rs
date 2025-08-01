//! P2P Blob API

mod bootstrap;
mod hash;
mod tag;

pub use crate::bootstrap::Bootstrapper;
pub use crate::hash::Hash;
pub use crate::tag::Tag;

use async_stream::stream;
use eyre::{Context, Result};
use futures::StreamExt;
use hash::HashGossipMsg;
use iroh::NodeId;
use iroh_gossip::api::{ApiError, GossipApi};
use iroh_gossip::proto::TopicId;
use serde::{Deserialize, Serialize};
use sha2::Digest as _;
use sha2::Sha256;
use tracing::{debug, error, warn};

// Used to disambiguate from other contexts/topics.
const HASH_CTX: &str = "orb-blob-v0";
const BOOTSTRAP_TOPIC: &str = "orb-blob-v0";

/// A reference to a particular blob.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub struct BlobRef([u8; 32]);

impl BlobRef {
    fn kind(&self) -> BlobRefKind {
        match self.0[0] >> 7 {
            // most significant bit indicates if its a hash or tag
            0 => BlobRefKind::Hash,
            1 => BlobRefKind::Tag,
            _ => unreachable!(),
        }
    }
}

impl From<&Tag> for BlobRef {
    fn from(value: &Tag) -> Self {
        let mut hasher: Sha256 = sha2::Digest::new();
        hasher.update(HASH_CTX);
        hasher.update("tag");

        hasher.update(value.domain.as_ref());
        hasher.update(&value.name);
        let mut hash: [u8; 32] = hasher.finalize().into();
        hash[0] |= 1 << 7; // Set MSB to 1, to indicate its a tag

        Self(hash)
    }
}

impl From<Hash> for BlobRef {
    fn from(value: Hash) -> Self {
        let mut hasher: Sha256 = sha2::Digest::new();
        hasher.update(HASH_CTX);
        hasher.update("hash");
        hasher.update(value.as_bytes());
        let mut hash: [u8; 32] = hasher.finalize().into();
        hash[0] &= u8::MAX >> 1; // set MSB to 0, to indicate its a hash

        Self(hash)
    }
}

#[derive(Debug, Eq, PartialEq, Clone, Copy)]
pub enum BlobRefKind {
    Tag,
    Hash,
}

#[cfg(test)]
mod test_blob_ref {
    use super::*;

    #[test]
    fn test_known_refs() {
        assert_eq!(BlobRef([0; 32]).kind(), BlobRefKind::Hash, "all zeros");
        assert_eq!(BlobRef([u8::MAX; 32]).kind(), BlobRefKind::Tag, "all ones");
        // TODO: Test more
    }
}

#[derive(Debug, Serialize, Deserialize)]
enum GossipMsg {
    Tag(crate::tag::TagGossipMsg),
    Hash(crate::hash::HashGossipMsg),
}

#[derive(Debug, bon::Builder, Clone)]
pub struct Client {
    my_node_id: NodeId, // Ideally we would figure this out directly from the underlying iroh
    // endpoint internal to the GossipApi
    gossip: GossipApi,
    bootstrap_nodes: Vec<NodeId>,
}

impl Client {
    pub async fn broadcast_to_peers(&self, blob: impl Into<BlobRef>) -> Result<()> {
        assert!(
            !self.bootstrap_nodes.is_empty(),
            "need at least 1 bootstrap node"
        );
        let blob_ref: BlobRef = blob.into();

        let bootstrap_topic = {
            let mut hasher: Sha256 = sha2::Digest::new();
            hasher.update(HASH_CTX);
            hasher.update(BOOTSTRAP_TOPIC);
            let hash: [u8; 32] = hasher.finalize().into();

            TopicId::from_bytes(hash)
        };
        debug!("broadcaster about to subscribe");
        let mut topic = self
            .gossip
            .subscribe_and_join(bootstrap_topic, self.bootstrap_nodes.clone())
            .await
            .wrap_err("failed to subscribe")?;
        debug!("broadcaster subscribed");

        let broadcast_msg = match blob_ref.kind() {
            BlobRefKind::Tag => todo!("tags are not yet supported"),
            BlobRefKind::Hash => GossipMsg::Hash(HashGossipMsg {
                blob_ref,
                node_id: self.my_node_id,
            }),
        };
        let serialized = serde_json::to_vec(&broadcast_msg).expect("infallible");
        topic
            .broadcast(serialized.into())
            .await
            .wrap_err("failed to broadcast to peers")?;
        debug!("broadcast successful");

        Ok(())
    }

    pub async fn listen_for_peers(
        &self,
        blob: impl Into<BlobRef>,
    ) -> Result<impl futures::Stream<Item = NodeId> + Unpin + Send + 'static> {
        assert!(
            !self.bootstrap_nodes.is_empty(),
            "need at least 1 bootstrap node"
        );
        let blob_ref: BlobRef = blob.into();

        let bootstrap_topic = {
            let mut hasher: Sha256 = sha2::Digest::new();
            hasher.update(HASH_CTX);
            hasher.update(BOOTSTRAP_TOPIC);
            let hash: [u8; 32] = hasher.finalize().into();

            TopicId::from_bytes(hash)
        };
        debug!("listener about to subscribe");
        let mut topic = self
            .gossip
            .subscribe_and_join(bootstrap_topic, self.bootstrap_nodes.clone())
            .await
            .wrap_err("failed to subscribe")?;
        debug!("listener subscribed");

        Ok(Box::pin(stream! {
            while let Some(result) = topic.next().await {
                let event = match result {
                    Err(ApiError::Closed { .. }) => break,
                    Ok(e) => e,
                    Err(err) => {
                        error!("error while listening to gossip topic: {err}");
                        break;
                    }
                };
                let iroh_gossip::api::Event::Received(msg) = event else {
                    continue;
                };

                let deserialized: Result<GossipMsg, _> =
                    serde_json::from_slice(msg.content.as_ref());
                let gossip_msg = match deserialized {
                    Err(err) => {
                        warn!("peer had invalid message: {err}");
                        continue;
                    }
                    Ok(deserialized) => deserialized,
                };

                let hash_gossip_msg = match gossip_msg {
                    GossipMsg::Tag(_) => todo!("we will implement tags later"),
                    GossipMsg::Hash(m) => m,
                };
                if hash_gossip_msg.blob_ref != blob_ref {
                    continue; // ignore refs that arent relevant to us
                }
                yield hash_gossip_msg.node_id;
            }
        }))
    }
}
