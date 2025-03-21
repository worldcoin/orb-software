use std::{
    io::IsTerminal as _,
    path::{Path, PathBuf},
};

use aws_sdk_s3::Client;
use color_eyre::{eyre::WrapErr as _, Result};
use futures::TryStreamExt as _;
use orb_s3_helpers::{ClientExt as _, S3Uri};
use tracing::info;

use crate::ota_path::OtaPath;

pub async fn fetch_path(client: &Client, path: &OtaPath) -> Result<PathBuf> {
    let download_dir = tempfile::tempdir_in(".")
        .wrap_err("failed to create temporary directory in current directoyr")?;

    match path {
        OtaPath::S3(s3_uri) => fetch_s3(client, s3_uri, download_dir.path()).await,
        OtaPath::Version(ota_version) => {
            fetch_s3(client, &ota_version.to_s3_uri(), download_dir.path()).await
        }
        OtaPath::Path(path_buf) => return Ok(path_buf.to_owned()),
    }
    .wrap_err("failed to download {path}")?;

    todo!()
}

/// Preconditions:
/// - `out_dir` should already exist
/// - `s3_dir.is_dir()` should be `true`.
async fn fetch_s3(client: &Client, s3_dir: &S3Uri, out_dir: &Path) -> Result<()> {
    assert!(s3_dir.is_dir(), "only directories should be provided");
    assert!(out_dir.exists(), "out_dir should exist");

    let objects: Vec<_> = client
        .list_prefix(s3_dir)
        .try_collect()
        .await
        .wrap_err("error while listing s3 dir")?;
    let total_ota_size: u64 = objects
        .iter()
        .map(|o| u64::try_from(o.size().unwrap_or_default()).expect("overflow"))
        .sum();

    let pb = std::io::stderr()
        .is_terminal()
        .then(|| indicatif::ProgressBar::new(total_ota_size));
    for o in objects {
        info!("key: {}", o.key().unwrap());
    }
    todo!()
}
