use crate::{
    cfg::Cfg,
    handlers::{blob, download, health, info},
};
use axum::{
    routing::{delete, get, post},
    Router,
};
use color_eyre::eyre::{eyre, Context, ContextCompat, Result};
use iroh::{protocol::Router as IrohRouter, Endpoint, RelayMode, Watcher};
use iroh_blobs::{store::fs::FsStore, BlobsProtocol};
use iroh_gossip::net::Gossip;
use orb_blob_p2p::{Bootstrapper, PeerTracker};
use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::{
    fs::{self, OpenOptions},
    net::TcpListener,
    sync::Mutex,
    task::{self, JoinHandle},
    time,
};
use tokio_util::sync::CancellationToken;

pub async fn run(
    cfg: Cfg,
    listener: TcpListener,
    shutdown: CancellationToken,
) -> Result<()> {
    let is_well_known_nodes_empty = cfg.well_known_nodes.is_empty();
    let _port = cfg.port;
    let deps = Deps::new(cfg).await?;
    let blob_store = deps.blob_store.clone();

    let blob_store_clone = deps.blob_store.clone();
    let tracker = deps.tracker.clone();
    let shutdown_broadcast = shutdown.child_token();

    let broadcast_task = async move {
        if !is_well_known_nodes_empty {
            broadcast_and_shit(tracker, blob_store_clone, shutdown_broadcast)
                .await
                .wrap_err("task panicked")?;
        }

        Ok(())
    };

    let serve_fut = async {
        let app = router(deps);
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                shutdown.cancelled().await;
                blob_store.sync_db().await.unwrap();
                blob_store.shutdown().await.unwrap();
            })
            .await
            .wrap_err("could not start axum 😱")
    };
    let ((), ()) =
        tokio::try_join!(serve_fut, broadcast_task).wrap_err("task failed")?;

    Ok(())
}

pub fn router(deps: Deps) -> Router {
    Router::new()
        .route("/health", get(health::handler))
        .route("/blob", post(blob::create))
        .route("/blob/{hash}", delete(blob::delete_by_hash))
        .route("/download", post(download::handler))
        .route("/info", get(info::handler))
        .with_state(deps)
}

#[derive(Clone)]
pub struct Deps {
    pub blob_store: Arc<FsStore>,
    pub sqlite: SqlitePool,
    pub tracker: PeerTracker,
    pub router: IrohRouter,
    pub cfg: Arc<Cfg>,
}

impl Deps {
    pub async fn new(cfg: Cfg) -> Result<Self> {
        let sqlite_path = cfg
            .sqlite_path
            .to_str()
            .wrap_err("could not get sqlite path")?;

        if !fs::try_exists(sqlite_path).await.unwrap_or(false) {
            OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(sqlite_path)
                .await
                .wrap_err_with(|| {
                    format!("failed to create empty sqlite file at {sqlite_path}")
                })?;
        }

        let sqlite = SqlitePoolOptions::new()
            .connect(sqlite_path)
            .await
            .wrap_err_with(|| format!("failed to open database at {sqlite_path}"))?;

        let blob_store = Arc::new(
            FsStore::load(&cfg.store_path)
                .await
                .map_err(|e| eyre!(e.to_string()))?,
        );

        let endpoint = Endpoint::builder()
            .clear_discovery()
            .secret_key(cfg.secret_key.clone());

        let endpoint = if cfg.iroh_local {
            endpoint
                .relay_mode(RelayMode::Disabled)
                .discovery_local_network()
                .bind_addr_v4("127.0.0.1:0".parse().expect("infallible"))
                .bind_addr_v6("[::1]:0".parse().expect("infallible"))
        } else {
            endpoint.discovery_dht().discovery_n0()
        };
        let endpoint = endpoint.bind().await?;

        if !cfg.iroh_local {
            let _relay_addr = tokio::time::timeout(
                Duration::from_millis(4000),
                endpoint.home_relay().initialized(),
            )
            .await
            .wrap_err("timed out waiting for home relay address")?;
        }

        let gossip = Gossip::builder().spawn(endpoint.clone());
        let blobs = BlobsProtocol::new(&blob_store, endpoint.clone(), None);
        let router = IrohRouter::builder(endpoint.clone())
            .accept(iroh_gossip::ALPN, gossip.clone())
            .accept(iroh_blobs::ALPN, blobs)
            .spawn();

        let boot = Bootstrapper {
            well_known_nodes: cfg.well_known_nodes.clone(),
            well_known_endpoints: vec![],
            use_dht: false,
        };

        let bootstrap_nodes = boot
            .find_bootstrap_peers(Duration::from_secs(5))
            .await
            .unwrap();

        let tracker = PeerTracker::builder()
            .gossip(&gossip)
            .endpoint(endpoint)
            .bootstrap_nodes(bootstrap_nodes)
            .build()
            .await
            .wrap_err("failed to create peer tracker")?;

        Ok(Deps {
            blob_store,
            sqlite,
            router,
            tracker,
            cfg: Arc::new(cfg),
        })
    }
}

fn broadcast_and_shit(
    tracker: PeerTracker,
    store: Arc<FsStore>,
    cancel: CancellationToken,
) -> JoinHandle<()> {
    let task_fut = async move {
        let broadcasted = Arc::new(Mutex::new(HashMap::new()));

        loop {
            let hashes = store.list().hashes().await.unwrap();

            for hash in hashes {
                if !broadcasted.lock().await.contains_key(&hash) {
                    let tracker_clone = tracker.clone();
                    let broadcasted_clone = broadcasted.clone();

                    let handle = task::spawn(async move {
                        if let Err(e) = tracker_clone.broadcast_to_peers(hash).await {
                            println!("{}", e.to_string().as_str())
                        };

                        broadcasted_clone.lock().await.remove(&hash);
                    });

                    broadcasted.lock().await.insert(hash, handle);
                }

                time::sleep(Duration::from_secs(1)).await;
            }
        }
    };

    task::spawn(async {
        let _ = cancel.run_until_cancelled_owned(task_fut).await;
    })
}
