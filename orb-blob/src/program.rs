use crate::{
    cfg::Cfg,
    handlers::{blob, health},
};
use axum::{
    routing::{get, post},
    Router,
};
use color_eyre::eyre::{eyre, Context, ContextCompat, Result};
use iroh_blobs::store::fs::FsStore;
use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};
use std::sync::Arc;
use tokio::net::TcpListener;

pub async fn run(cfg: Cfg, listener: TcpListener) -> Result<()> {
    let state = Deps::new(&cfg).await?;
    let app = router(state);

    println!("Listening on port {}", cfg.port);
    axum::serve(listener, app)
        .await
        .wrap_err("could not start axum 😱")
}

pub fn router(deps: Deps) -> Router {
    Router::new()
        .route("/health", get(health::handler))
        .route("/blob", post(blob::create))
        .with_state(deps)
}

#[derive(Clone)]
pub struct Deps {
    #[expect(unused)]
    pub blob_store: Arc<FsStore>,
    #[expect(unused)]
    pub sqlite: SqlitePool,
}

impl Deps {
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

        let blob_store = Arc::new(
            FsStore::load(&cfg.store_path)
                .await
                .map_err(|e| eyre!(e.to_string()))?,
        );

        Ok(Deps { blob_store, sqlite })
    }
}
