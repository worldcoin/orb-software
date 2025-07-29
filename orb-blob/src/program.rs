use crate::{cfg::Cfg, handlers::health};
use axum::{routing::get, Router};
use color_eyre::eyre::{Context, Result};
use tokio::net::TcpListener;

pub async fn run(cfg: Cfg, listener: TcpListener) -> Result<()> {
    let app = Router::new().route("/health", get(health::handler));

    axum::serve(listener, app)
        .await
        .wrap_err("could not start axum 😱")
}
