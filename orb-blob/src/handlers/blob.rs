use crate::program::Deps;
use axum::http::StatusCode;
use axum::{extract::State, Json};
use color_eyre::Result;
use serde::{Deserialize, Serialize};
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
