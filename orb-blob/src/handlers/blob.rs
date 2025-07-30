use crate::program::Deps;
use axum::http::StatusCode;
use axum::{
    extract::{Path, State},
    Json,
};
use iroh_blobs::Hash;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs::{self, File};
use tokio::io::AsyncWriteExt;

use color_eyre::Result;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
pub struct CreateReq {
    path: String,
}

pub async fn create(
    State(deps): State<Deps>,
    Json(req): Json<CreateReq>,
) -> Result<StatusCode, String> {
    let result: Result<()> = async move {
        let abs_path = fs::canonicalize(req.path).await?;
        deps.blob_store.blobs().add_path(abs_path).await?;
        Ok(())
    }
    .await;

    match result {
        Ok(_) => Ok(StatusCode::CREATED),
        Err(e) => Err(e.to_string()),
    }
}
