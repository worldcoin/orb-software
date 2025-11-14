use crate::program::Deps;
use axum::extract::State;
use axum::http::StatusCode;
use color_eyre::Result;
use iroh::node_info::NodeIdExt;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[allow(dead_code)]
#[derive(Deserialize, Serialize)]
pub struct CreateReq {
    path: String,
}

pub async fn handler(
    State(deps): State<Deps>,
) -> Result<(StatusCode, String), (StatusCode, String)> {
    let result: Result<String> = async move {
        let cfg = &deps.cfg;

        let store_path = &cfg.store_path;
        let sqlite_path = &cfg.sqlite_path;
        let timetout = cfg.peer_listen_timeout;
        let min_peers = cfg.min_peer_req;
        let pub_key = cfg.secret_key.public();
        let known_nodes = &cfg.well_known_nodes;
        let hashes = deps.blob_store.list().hashes().await?;

        let response = json!(
            {
                "store_path": store_path,
                "sqilte_path": sqlite_path,
                "timeout": timetout,
                "min_peers": min_peers,
                "pub_key": pub_key.to_z32(),
                "known_nodes": known_nodes,
                "hashes": hashes
            }
        );
        Ok(response.to_string())
    }
    .await;

    match result {
        Ok(blob_info) => Ok((StatusCode::OK, blob_info)),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}
