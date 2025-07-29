use crate::{cfg::Cfg, handlers::health};
use axum::{routing::get, Router};
use color_eyre::eyre::{eyre, Context, ContextCompat, Result};
use iroh_blobs::store::fs::FsStore;
use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};
use std::sync::Arc;
use tokio::net::TcpListener;

pub async fn run(cfg: Cfg, listener: TcpListener) -> Result<()> {
    let state = State::new(&cfg).await?;
    let app = router(state);

    axum::serve(listener, app)
        .await
        .wrap_err("could not start axum 😱")
}

pub fn router(state: State) -> Router {
    Router::new()
        .route("/health", get(health::handler))
        .with_state(Arc::new(state))
}

pub struct State {
    blob_store: FsStore,
    sqlite: SqlitePool,
}

impl State {
    pub async fn new(cfg: &Cfg) -> Result<Self> {
        let sqlite_path = cfg
            .sqlite_path
            .to_str()
            .wrap_err("could not get sqlite path")?;

        if !tokio::fs::try_exists(sqlite_path).await.unwrap_or(false) {
            tokio::fs::OpenOptions::new()
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
        let blob_store = FsStore::load(&cfg.store_path)
            .await
            .map_err(|e| eyre!(e.to_string()))?;

        Ok(State { blob_store, sqlite })
    }
}
