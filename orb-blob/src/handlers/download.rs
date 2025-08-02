use crate::program::Deps;
use axum::http::StatusCode;
use axum::{extract::State, Json};
use color_eyre::eyre::{eyre, Context, Result};
use iroh_blobs::api::downloader::Shuffled;
use iroh_blobs::Hash;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::str::FromStr;
use tokio::time::{self};

#[derive(Deserialize, Serialize)]
pub struct DownloadReq {
    hash: String,
    download_path: String,
}

#[axum::debug_handler]
pub async fn handler(
    State(deps): State<Deps>,
    Json(req): Json<DownloadReq>,
) -> Result<StatusCode, (StatusCode, String)> {
    let result: Result<_> = async move {
        let hash = Hash::from_str(&req.hash)?;

        let peers_fut = async {
            let mut peer_watcher = deps.tracker.listen_for_peers(hash).await;
            let peers_ref = peer_watcher
                .wait_for(|peer_set| peer_set.len() >= deps.cfg.min_peer_req)
                .await
                .expect("shouldn't be cancelled as long as client exists");
            // we clone because `Ref<T>` is !Send
            let peers: HashSet<_> = peers_ref.clone();

            peers
        };
        let peers = time::timeout(deps.cfg.peer_listen_timeout, peers_fut)
            .await
            .wrap_err("timed out waiting for enough peers")?;

        let downloader = deps.blob_store.downloader(deps.router.endpoint());
        downloader
            .download(hash, Shuffled::new(peers.into_iter().collect()))
            .await
            .map_err(|e| eyre!(e.to_string()))?;

        deps.blob_store
            .blobs()
            .export(hash, req.download_path)
            .await?;

        Ok(())
    }
    .await;

    match result {
        Ok(()) => Ok(StatusCode::CREATED),
        Err(e) => Err((StatusCode::NOT_FOUND, e.to_string())),
    }
}
