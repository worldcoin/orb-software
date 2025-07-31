use color_eyre::Result;
use eyre::Context;
use futures::StreamExt;
use iroh::{protocol::Router, Endpoint, NodeId, SecretKey};
use iroh_gossip::net::Gossip;
use orb_blob_p2p::{BlobTopic, Client, Hash, HashTopic};
use rand::{RngCore, SeedableRng};

#[tokio::test]
async fn main() -> Result<()> {
    color_eyre::install()?;
    tracing_subscriber::fmt().init();

    let Nodes { bootstrap, a, b } =
        setup_nodes().await.wrap_err("failed to set up nodes")?;

    let topic = BlobTopic::Hash(HashTopic {
        hash: Hash(iroh_blobs::Hash::EMPTY),
    });

    let peers_a = a
        .p2p
        .listen_for_peers(topic.clone())
        .await
        .wrap_err("`a` failed to listen")?;

    b.p2p
        .broadcast_to_peers(topic)
        .await
        .wrap_err("`b` failed to broadcast")?;

    // while let Some(p) = peers_a.next().await {
    //
    // }

    Ok(())
}

struct Nodes {
    bootstrap: Spawned,
    a: Spawned,
    b: Spawned,
}

async fn setup_nodes() -> Result<Nodes> {
    let mut node_keys = example_keys(3);
    let bootstrap = spawn_node(node_keys.pop().unwrap(), None)
        .await
        .wrap_err("failed to spawn boostrap node")?;
    let bootstrap_node_id = bootstrap.endpoint.node_id();
    let mut spawned = futures::future::try_join_all(
        node_keys
            .into_iter()
            .map(|k| spawn_node(k, Some(bootstrap_node_id))),
    )
    .await
    .wrap_err("failed to spawn nodes")?;
    let a = spawned.pop().unwrap();
    let b = spawned.pop().unwrap();
    assert!(spawned.is_empty(), "dont spawn more than we need");

    Ok(Nodes { bootstrap, a, b })
}

struct Spawned {
    endpoint: Endpoint,
    gossip: Gossip,
    router: Router,
    p2p: Client,
}

async fn spawn_node(
    secret_key: SecretKey,
    bootstrap: Option<NodeId>,
) -> Result<Spawned> {
    let endpoint = iroh::Endpoint::builder()
        .relay_mode(iroh::RelayMode::Disabled) // we are testing locally
        .secret_key(secret_key)
        .bind()
        .await
        .wrap_err("failed to bind to endpoint")?;
    let gossip = iroh_gossip::net::Gossip::builder().spawn(endpoint.clone());
    let router = iroh::protocol::Router::builder(endpoint.clone())
        .accept(iroh_gossip::ALPN, gossip.clone())
        .spawn();

    let p2p = Client::builder()
        .gossip((*gossip).clone())
        .my_node_id(endpoint.node_id())
        .bootstrap_nodes(bootstrap.map(|b| Vec::from([b])).unwrap_or_default())
        .build();

    Ok(Spawned {
        endpoint,
        gossip,
        router,
        p2p,
    })
}

fn example_keys(n: u8) -> Vec<SecretKey> {
    const SEED: u64 = 1337; // seed for reproducibility of tests
    let mut rng = rand::rngs::StdRng::seed_from_u64(SEED);

    let mut bytes = [0; 32];
    (0..n)
        .map(|_| {
            rng.fill_bytes(&mut bytes);
            SecretKey::from_bytes(&bytes)
        })
        .collect()
}
