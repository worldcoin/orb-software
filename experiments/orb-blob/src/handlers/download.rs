#![allow(clippy::uninlined_format_args)]
use crate::program::Deps;
use axum::http::StatusCode;
use axum::{extract::State, Json};
use color_eyre::eyre::{eyre, Context, Result};
use futures_lite::StreamExt;
use iroh_blobs::api::downloader::DownloadProgressItem;
use iroh_blobs::api::downloader::Shuffled;
use iroh_blobs::Hash;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::str::FromStr;
use tokio::io::{self, AsyncWriteExt};
use tokio::time::{self};
use tracing::warn;

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
        for peer in peers.iter() {
            if let Some(remote_info) = deps.router.endpoint().remote_info(*peer).await {
                for addr_info in remote_info.addrs() {
                    warn!("peer {peer:?} addr: {:?}", addr_info);
                }
            } else {
                warn!("no remote info for peer {peer:?}");
            }
        }

        let downloader = deps.blob_store.downloader(deps.router.endpoint());
        let mut progress = downloader
            .download(hash, Shuffled::new(peers.into_iter().collect()))
            .stream()
            .await
            .map_err(|e| eyre!(e.to_string()))?;

        let mut stdout = io::stdout();
        while let Some(item) = progress.next().await {
            match item {
                DownloadProgressItem::Progress(bytes_downloaded) => {
                    let line =
                        format!("\rDownloaded {} KB so far", bytes_downloaded / 1024);
                    stdout.write_all(line.as_bytes()).await?;
                    stdout.flush().await?;
                }

                // NOTE: leaving the errors intact so that it's more visible that it's still
                // connected
                DownloadProgressItem::ProviderFailed { id, .. } => {
                    eprintln!("Provider {} failed", id);
                }
                DownloadProgressItem::DownloadError => {
                    eprintln!("A part failed to download");
                }
                DownloadProgressItem::Error(err) => {
                    eprintln!("Fatal error: {}", err);
                    break;
                }
                _ => {}
            }
        }

        deps.blob_store
            .blobs()
            .export(hash, req.download_path)
            .await?;

        Ok(())
    }
    .await;

    match result {
        Ok(()) => Ok(StatusCode::CREATED),
        Err(e) => Err((StatusCode::NOT_FOUND, dbg!(e.to_string()))),
    }
}
