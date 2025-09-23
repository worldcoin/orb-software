#![allow(clippy::uninlined_format_args)]
use crate::program::Deps;
use axum::http::StatusCode;
use axum::{extract::State, Json};
use color_eyre::eyre::{eyre, Context, Result};
use futures_lite::StreamExt;
use iroh::Watcher;
use iroh_blobs::api::downloader::DownloadProgessItem;
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
            let Some(peer) = deps.router.endpoint().conn_type(*peer) else {
                warn!("no conn type");
                continue;
            };
            let mut stream = peer.stream();
            tokio::spawn(async move {
                while let Some(evt) = stream.next().await {
                    match evt {
                        iroh::endpoint::ConnectionType::Direct(_) => {
                            warn!("using direct connection")
                        }
                        iroh::endpoint::ConnectionType::Relay(_) => {
                            warn!("using relayed/tunneled connection")
                        }
                        iroh::endpoint::ConnectionType::Mixed(..) => {
                            warn!("using mixed connection")
                        }
                        iroh::endpoint::ConnectionType::None => warn!("no connection"),
                    }
                }
            });
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
                DownloadProgessItem::Progress(bytes_downloaded) => {
                    let line =
                        format!("\rDownloaded {} KB so far", bytes_downloaded / 1024);
                    stdout.write_all(line.as_bytes()).await?;
                    stdout.flush().await?;
                }

                // NOTE: leaving the errors intact so that it's more visible that it's still
                // connected
                DownloadProgessItem::ProviderFailed { id, .. } => {
                    eprintln!("Provider {} failed", id);
                }
                DownloadProgessItem::DownloadError => {
                    eprintln!("A part failed to download");
                }
                DownloadProgessItem::Error(err) => {
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
