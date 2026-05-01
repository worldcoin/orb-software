use std::time::Duration;

use color_eyre::Result;
use eyre::Context;
use iroh::{EndpointAddr, EndpointId, SecretKey};
use orb_blob_p2p::{BlobRef, PeerTracker};
use tokio_util::sync::CancellationToken;
use tracing::info;

// macos-15 runner doesn't allow multicast https://github.com/actions/runner-images/issues/10924
#[cfg_attr(target_os = "macos", test_with::no_env(GITHUB_ACTIONS))]
#[tokio::test]
async fn main() -> Result<()> {
    color_eyre::install()?;
    tracing_subscriber::fmt().init();

    let Nodes {
        bootstrap: _bootstrap,
        a,
        b,
    } = setup_nodes().await.wrap_err("failed to set up nodes")?;

    let blob_ref = BlobRef::from(iroh_blobs::Hash::EMPTY);

    let cancel = CancellationToken::new();
    let b_p2p = b.p2p.clone();
    let broadcaster_fut = cancel.child_token().run_until_cancelled_owned(async move {
        loop {
            tokio::time::timeout(
                Duration::from_millis(1000),
                b_p2p.broadcast_to_peers(blob_ref),
            )
            .await
            .wrap_err("timed out while attempting to broacast from `b`")
            .unwrap()
            .wrap_err("`b` failed to broadcast")
            .unwrap();
            tokio::time::sleep(Duration::from_millis(1000)).await;
        }
    });

    let listen_fut = async move {
        let mut peers_a = a.p2p.listen_for_peers(blob_ref).await;
        tokio::time::timeout(
            Duration::from_secs(10),
            peers_a.wait_for(|endpoint_id_set| {
                endpoint_id_set.contains(&b.p2p.endpoint().id())
            }),
        )
        .await
        .expect("timed out waiting for peer")
        .expect("watch channel errored out");
        cancel.cancel();
    };
    let broadcaster_task = tokio::spawn(broadcaster_fut);
    let listen_task = tokio::spawn(listen_fut);

    let _ = tokio::try_join!(broadcaster_task, listen_task).wrap_err("failed task")?;

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
    let bootstrap_endpoint_id = bootstrap.p2p.endpoint().id();
    let mut spawned = futures::future::try_join_all(
        node_keys
            .into_iter()
            .map(|k| spawn_node(k, Some(bootstrap_endpoint_id))),
    )
    .await
    .wrap_err("failed to spawn nodes")?;
    let a = spawned.pop().unwrap();
    let b = spawned.pop().unwrap();
    assert!(spawned.is_empty(), "dont spawn more than we need");

    Ok(Nodes { bootstrap, a, b })
}

struct Spawned {
    _router: iroh::protocol::Router,
    p2p: PeerTracker,
}

async fn spawn_node(
    secret_key: SecretKey,
    bootstrap: Option<EndpointId>,
) -> Result<Spawned> {
    use iroh::endpoint::presets;

    let endpoint = iroh::Endpoint::builder(presets::Empty)
        .relay_mode(iroh::RelayMode::Disabled) // we are testing locally
        .secret_key(secret_key)
        .bind_addr("127.0.0.1:0".parse::<std::net::SocketAddrV4>().unwrap())?
        .bind_addr("[::1]:0".parse::<std::net::SocketAddrV6>().unwrap())?
        .clear_address_lookup()
        .bind()
        .await
        .wrap_err("failed to bind to endpoint")?;
    info!(
        "my endpoint id is {} (is_bootstrap={})",
        endpoint.id(),
        bootstrap.is_none()
    );
    info!("bound sockets: {:?}", endpoint.bound_sockets());

    let gossip = iroh_gossip::net::Gossip::builder().spawn(endpoint.clone());
    let router = iroh::protocol::Router::builder(endpoint.clone())
        .accept(iroh_gossip::ALPN, gossip.clone())
        .spawn();

    let p2p = PeerTracker::builder()
        .gossip(&gossip)
        .endpoint(endpoint)
        .bootstrap_nodes(
            bootstrap
                .map(|b| Vec::from([EndpointAddr::from(b)]))
                .unwrap_or_default(),
        )
        .build()
        .await
        .wrap_err("failed to create client")?;

    Ok(Spawned {
        p2p,
        _router: router,
    })
}

fn example_keys(n: u8) -> Vec<SecretKey> {
    (0..n).map(|i| SecretKey::from_bytes(&[i; 32])).collect()
}
