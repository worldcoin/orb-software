//! P2P Blob API

mod bootstrap;
mod hash;
mod tag;

use std::collections::{HashMap, HashSet};

pub use crate::bootstrap::Bootstrapper;
pub use crate::hash::Hash;
pub use crate::tag::Tag;

use eyre::{Context, Result};
use futures::StreamExt;
use hash::HashGossipMsg;
use iroh::NodeId;
use iroh_gossip::api::{ApiError, GossipApi, GossipReceiver, GossipSender};
use iroh_gossip::proto::TopicId;
use serde::{Deserialize, Serialize};
use sha2::Digest as _;
use sha2::Sha256;
use tokio::sync::{oneshot, watch};
use tracing::{debug, error, warn};

// Used to disambiguate from other contexts/topics.
const HASH_CTX: &str = "orb-blob-v0";
const BOOTSTRAP_TOPIC: &str = "orb-blob-v0";

/// A reference to a particular blob.
#[derive(
    Debug, Clone, Copy, Eq, PartialEq, Hash, Ord, PartialOrd, Serialize, Deserialize,
)]
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

type RegisterMsg = (BlobRef, oneshot::Sender<watch::Receiver<WatchMsg>>);
type WatchMsg = HashSet<NodeId>; // TODO: Use a datastructure that can evict stale NodeIds

#[derive(Debug, Clone)]
pub struct Client {
    my_node_id: NodeId, // Ideally we would figure this out directly from the underlying iroh
    // endpoint internal to the GossipApi
    _gossip: GossipApi,
    topic_sender: GossipSender,
    bootstrap_nodes: Vec<NodeId>,
    register_tx: flume::Sender<RegisterMsg>,
}

#[bon::bon]
impl Client {
    /// If any bootstrap nodes are provided, will wait until successfully connecting to
    /// them.
    #[builder]
    pub async fn new(
        my_node_id: NodeId,
        gossip: GossipApi,
        bootstrap_nodes: Vec<NodeId>,
    ) -> Result<Self> {
        let (register_tx, register_rx) = flume::unbounded();
        let bootstrap_topic = {
            let mut hasher: Sha256 = sha2::Digest::new();
            hasher.update(HASH_CTX);
            hasher.update(BOOTSTRAP_TOPIC);
            let hash: [u8; 32] = hasher.finalize().into();

            TopicId::from_bytes(hash)
        };

        let topic = if bootstrap_nodes.is_empty() {
            debug!("topic about to join topic (nonblocking)");
            gossip
                .subscribe(bootstrap_topic, bootstrap_nodes.clone())
                .await
        } else {
            debug!("topic about to join topic (blocking)");
            gossip
                .subscribe_and_join(bootstrap_topic, bootstrap_nodes.clone())
                .await
        }
        .wrap_err("failed to subscribe")?;
        debug!("joined topic");

        let (topic_sender, topic_receiver) = topic.split();

        tokio::spawn(
            listen_task()
                .topic(topic_receiver)
                .register_rx(register_rx)
                .call(),
        );

        Ok(Self {
            my_node_id,
            _gossip: gossip,
            topic_sender,
            bootstrap_nodes,
            register_tx,
        })
    }

    pub async fn listen_for_peers(
        &self,
        blob: impl Into<BlobRef>,
    ) -> watch::Receiver<WatchMsg> {
        let (tx, rx) = oneshot::channel();
        self.register_tx
            .send_async((blob.into(), tx))
            .await
            .expect("unbounded channel, no send error possible unless task crashed");

        rx.await.expect("not possible to fail unless task crashed")
    }

    pub async fn broadcast_to_peers(&self, blob: impl Into<BlobRef>) -> Result<()> {
        assert!(
            !self.bootstrap_nodes.is_empty(),
            "need at least 1 bootstrap node"
        );
        let blob_ref: BlobRef = blob.into();

        let broadcast_msg = match blob_ref.kind() {
            BlobRefKind::Tag => todo!("tags are not yet supported"),
            BlobRefKind::Hash => GossipMsg::Hash(HashGossipMsg {
                blob_ref,
                node_id: self.my_node_id,
            }),
        };
        let serialized = serde_json::to_vec(&broadcast_msg).expect("infallible");
        debug!("beginning broadcast");
        self.topic_sender
            .broadcast(serialized.into())
            .await
            .wrap_err("failed to broadcast to peers")?;
        debug!("broadcast successful");

        Ok(())
    }
}

#[bon::builder]
async fn listen_task(
    mut topic: GossipReceiver,
    register_rx: flume::Receiver<RegisterMsg>,
) -> Result<()> {
    let mut registry: HashMap<BlobRef, watch::Sender<WatchMsg>> = HashMap::new();

    loop {
        tokio::select! {
            register_result = register_rx.recv_async() => {
                let Ok((blob_ref, register_response_tx)) = register_result else {
                    // all senders i.e. clients have been dropped, terminate the future.
                    break;
                };
                let watch_rx = if let Some(watch_tx) = registry.get_mut(&blob_ref) {
                    watch_tx.subscribe()
                } else {
                    let (watch_tx, watch_rx) = watch::channel(WatchMsg::default());
                    registry.insert(blob_ref, watch_tx);

                    watch_rx
                };

                let _ = register_response_tx.send(watch_rx);
            }
            listen_result = topic.next() => {
                let Some(listen_result) = listen_result else {
                    break;
                };
                let event = match listen_result {
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
                let Some(watch_tx) = registry.get(&hash_gossip_msg.blob_ref).cloned()
                else {
                    continue; // ignore refs that dont have a registered listener
                };
                if watch_tx.is_closed() {
                    // prune registrations that have been dropped
                    registry.remove(&hash_gossip_msg.blob_ref);
                }
                watch_tx.send_modify(|peers| {
                    peers.insert(hash_gossip_msg.node_id);
                })
            }
        }
    }

    Ok(())
}
