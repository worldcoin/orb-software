use crate::{
    cfg::Cfg,
    handlers::{blob, health},
};
use axum::{
    routing::{delete, get, post},
    Router,
};

use color_eyre::eyre::{eyre, Context, ContextCompat, Result};
use iroh_blobs::store::fs::FsStore;

use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};
use std::sync::Arc;
use tokio::{
    fs::{self, OpenOptions},
    net::TcpListener,
};
use tokio_util::sync::CancellationToken;

pub async fn run(
    cfg: Cfg,
    listener: TcpListener,
    shutdown: CancellationToken,
) -> Result<()> {
    let deps = Deps::new(&cfg).await?;
    let blob_store = deps.blob_store.clone();

    let app = router(deps);

    println!("Listening on port {}", cfg.port);
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
        .with_state(deps)
}

#[derive(Clone)]
pub struct Deps {
    pub blob_store: Arc<FsStore>,
    pub sqlite: SqlitePool,
}

impl Deps {
    pub async fn new(cfg: &Cfg) -> Result<Self> {
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

        Ok(Deps { blob_store, sqlite })
    }
}
