use crate::{
    cfg::Cfg,
    handlers::{blob, download, health},
};
use axum::{
    routing::{delete, get, post},
    Router,
};
use color_eyre::eyre::{eyre, Context, ContextCompat, Result};
use iroh::{protocol::Router as IrohRouter, Endpoint};
use iroh_blobs::store::fs::FsStore;
use iroh_gossip::net::Gossip;
use orb_blob_p2p::{Bootstrapper, Client, HashTopic};
use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};
use std::{ops::Deref, sync::Arc, time::Duration};
use tokio::{
    fs::{self, OpenOptions},
    net::TcpListener,
    task, time,
};
use tokio_util::sync::CancellationToken;

pub async fn run(
    cfg: Cfg,
    listener: TcpListener,
    shutdown: CancellationToken,
) -> Result<()> {
    let port = cfg.port;
    let deps = Deps::new(cfg).await?;
    let blob_store = deps.blob_store.clone();

    broadcast_and_shit(deps.p2pclient.clone(), deps.blob_store.clone());
    let app = router(deps);

    println!("Listening on port {port}");
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown.cancelled().await;
            blob_store.sync_db().await.unwrap();
            blob_store.shutdown().await.unwrap();
        })
        .await
        .wrap_err("could not start axum 😱")
}

pub fn router(deps: Deps) -> Router {
    Router::new()
        .route("/health", get(health::handler))
        .route("/blob", post(blob::create))
        .route("/blob/{hash}", delete(blob::delete_by_hash))
        .route("/download", post(download::handler))
        .with_state(deps)
}

#[derive(Clone)]
pub struct Deps {
    pub blob_store: Arc<FsStore>,
    pub sqlite: SqlitePool,
    pub p2pclient: Client,
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
            .secret_key(cfg.secret_key.clone())
            .discovery_n0()
            .bind()
            .await?;

        let gossip = Gossip::builder().spawn(endpoint.clone());
        let router = IrohRouter::builder(endpoint.clone())
            .accept(iroh_gossip::ALPN, gossip.clone())
            .spawn();

        let boot = Bootstrapper {
            well_known_nodes: vec![],
            well_known_endpoints: vec![],
            use_dht: false,
        };

        let bootstrap_nodes = boot
            .find_bootstrap_peers(Duration::from_secs(5))
            .await
            .unwrap();

        let p2pclient = Client::builder()
            .my_node_id(cfg.secret_key.public())
            .gossip(gossip.deref().clone())
            .bootstrap_nodes(bootstrap_nodes.into_iter().collect())
            .build();

        Ok(Deps {
            blob_store,
            sqlite,
            router,
            p2pclient,
            cfg: Arc::new(cfg),
        })
    }
}

fn broadcast_and_shit(p2pclient: Client, store: Arc<FsStore>) {
    task::spawn(async move {
        loop {
            let hashes = store.list().hashes().await.unwrap();

            for hash in hashes {
                let hash_topic = HashTopic {
                    hash: orb_blob_p2p::Hash(hash),
                };

                if let Err(e) = p2pclient.broadcast_to_peers(hash_topic).await {
                    println!("{}", e.to_string().as_str())
                }
            }

            time::sleep(Duration::from_secs(1)).await;
        }
    });
}
