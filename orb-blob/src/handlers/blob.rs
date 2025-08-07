use crate::program::Deps;
use axum::http::StatusCode;
use axum::{
    extract::{Path, State},
    Json,
};
use color_eyre::Result;
use futures_lite::stream::StreamExt;
use iroh_blobs::Hash;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use tokio::fs::{self};

#[derive(Deserialize, Serialize)]
pub struct CreateReq {
    path: String,
}

pub async fn create(
    State(deps): State<Deps>,
    Json(req): Json<CreateReq>,
) -> Result<(StatusCode, String), String> {
    let result: Result<_> = async move {
        let abs_path = fs::canonicalize(req.path).await?;
        let tag_info = deps
            .blob_store
            .blobs()
            .add_path(abs_path)
            .with_tag()
            .await?;

        Ok(tag_info)
    }
    .await;

    match result {
        Ok(taginfo) => Ok((StatusCode::CREATED, taginfo.hash.to_string())),
        Err(e) => Err(e.to_string()),
    }
}

pub async fn delete_by_hash(
    State(deps): State<Deps>,
    Path(hash): Path<String>,
) -> Result<StatusCode, String> {
    let hash = Hash::from_str(&hash).map_err(|e| e.to_string())?;

    let tags = deps.blob_store.tags();

    let mut tags_stream = tags.list().await.map_err(|e| e.to_string())?;

    let mut found = false;

    while let Some(tag_info_res) = tags_stream.next().await {
        let tag_info = tag_info_res.map_err(|e| e.to_string())?;

        if tag_info.hash == hash {
            tags.delete(tag_info.name.clone())
                .await
                .map_err(|e| e.to_string())?;
            found = true;
        }
    }

    if found {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(StatusCode::NOT_FOUND
            .canonical_reason()
            .unwrap_or("not found")
            .to_string())
    }
}
