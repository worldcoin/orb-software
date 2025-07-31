use crate::program::Deps;
use axum::http::StatusCode;
use axum::{extract::State, Json};
use color_eyre::eyre::{eyre, Result};
use futures_lite::StreamExt;
use iroh_blobs::Hash;
use orb_blob_p2p::{Hash as OrbBlobHash, HashTopic};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use tokio::time::{self};

#[derive(Deserialize, Serialize)]
pub struct DownloadReq {
    hash: String,
    download_path: String,
}

pub async fn handler(
    State(deps): State<Deps>,
    Json(req): Json<DownloadReq>,
) -> Result<StatusCode, (StatusCode, String)> {
    let result: Result<_> = async move {
        let hash = Hash::from_str(&req.hash)?;
        let hash_topic = HashTopic {
            hash: OrbBlobHash(hash.clone()),
        };

        let mut peers = Vec::new();
        let mut peer_stream = deps.p2pclient.listen_for_peers(hash_topic).await?;

        let gather_peers = async {
            loop {
                if let Some(peer) = peer_stream.next().await {
                    peers.push(peer);
                }

                if peers.len() >= deps.cfg.min_peer_req {
                    break;
                }
            }
        };

        time::timeout(deps.cfg.peer_listen_timeout, gather_peers).await?;
        // TODO: freak out is 0 peers

        let downloader = deps.blob_store.downloader(deps.router.endpoint());
        downloader
            .download(hash.clone(), peers)
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
