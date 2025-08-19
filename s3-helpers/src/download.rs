#![allow(clippy::uninlined_format_args)]
use std::{ops::RangeInclusive, os::unix::fs::FileExt, sync::Arc, time::Duration};

use aws_sdk_s3::Client;
use camino::Utf8Path;
use color_eyre::{
    eyre::{ensure, eyre, WrapErr},
    Result,
};
use tokio::{sync::Mutex, task::JoinSet, time::timeout};
use tracing::warn;

use crate::{ExistingFileBehavior, S3Uri};

const PART_SIZE: u64 = 25 * 1024 * 1024; // 25 MiB
const CONCURRENCY: usize = 16;
const PART_DOWNLOAD_TIMEOUT_SECS: u64 = 120;
const PART_DOWNLOAD_NUM_RETRY: u8 = 5;

#[derive(Debug, Clone)]
struct ContentRange(RangeInclusive<u64>);

impl std::fmt::Display for ContentRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let start = self.0.start();
        let end = self.0.end();
        write!(f, "bytes={}-{}", start, end)
    }
}

#[derive(Debug, Default)]
pub struct Progress {
    pub bytes_so_far: u64,
    pub total_to_download: u64,
}

pub(crate) async fn download_multipart(
    client: &Client,
    object: &S3Uri,
    out_path: &Utf8Path,
    existing_file_behavior: ExistingFileBehavior,
    mut progress: impl FnMut(Progress),
) -> Result<()> {
    if existing_file_behavior == ExistingFileBehavior::Abort {
        ensure!(!out_path.exists(), "{out_path:?} already exists!");
    }
    let parent_dir = out_path
        .parent()
        .expect("please provide the path to a file")
        .to_owned();
    ensure!(
        parent_dir.try_exists().unwrap_or(false),
        "parent directory {parent_dir} doesn't exist"
    );

    let head_resp = client
        .head_object()
        .bucket(&object.bucket)
        .key(&object.key)
        .send()
        .await
        .wrap_err("failed to make aws head_object request")?;

    let bytes_to_download = head_resp.content_length().unwrap();

    let bytes_to_download: u64 = bytes_to_download
        .try_into()
        .expect("Download size is too large to fit into u64");

    let step_size = PART_SIZE
        .try_into()
        .expect("PART_SIZE is too large to fit into usize");
    let ranges = (0..bytes_to_download).step_by(step_size).map(move |start| {
        let end = std::cmp::min(start + PART_SIZE - 1, bytes_to_download - 1);
        ContentRange(start..=end)
    });

    let (tmp_file, tmp_file_path) = tokio::task::spawn_blocking(move || {
        let tmp_file = tempfile::NamedTempFile::new_in(parent_dir)
            .wrap_err("failed to create tempfile")?;
        tmp_file.as_file().set_len(bytes_to_download)?;
        Ok::<_, color_eyre::Report>(tmp_file.into_parts())
    })
    .await?
    .wrap_err("failed to create tempfile")?;

    let tmp_file: Arc<std::fs::File> = Arc::new(tmp_file);
    let tmp_file_path = Arc::new(tmp_file_path);

    let ranges = Arc::new(Mutex::new(ranges));
    let mut download_tasks = JoinSet::new();
    let (progress_tx, mut progress_rx) = tokio::sync::mpsc::unbounded_channel();
    for _ in 0..CONCURRENCY {
        let ranges = Arc::clone(&ranges);
        let client = client.clone();
        let object = object.clone();
        let tmp_file = Arc::clone(&tmp_file);
        let progress_tx = progress_tx.clone();

        download_tasks.spawn(async move {
            loop {
                let range_option = {
                    let mut ranges_lock = ranges.lock().await;
                    ranges_lock.next()
                };

                let Some(range) = range_option else {
                    break;
                };

                let body =
                    download_part_retry_on_timeout(&range, &client, &object).await?;
                let chunk_size = body.len() as u64;
                ensure!(
                    usize::try_from(chunk_size).unwrap() == range.0.clone().count(),
                    "downloaded bytes did not match range length"
                );

                let range_clone = range.clone();
                tokio::task::spawn_blocking({
                    let tmp_file = Arc::clone(&tmp_file);
                    move || {
                        tmp_file.write_all_at(&body, *range_clone.0.start())?;
                        Ok::<(), std::io::Error>(())
                    }
                })
                .await??;

                if let Err(_err) = progress_tx.send(range) {
                    // channel closed, terminate task
                    break;
                }
            }

            Ok::<(), color_eyre::Report>(())
        });
    }
    drop(progress_tx); // Without this, receiver will never be cancelled

    let mut bytes_so_far = 0;
    while let Some(range) = progress_rx.recv().await {
        // range is inclusive so we need to add 1
        bytes_so_far = std::cmp::max(bytes_so_far, *range.0.end() + 1);
        progress(Progress {
            bytes_so_far,
            total_to_download: bytes_to_download,
        })
    }
    ensure!(
        bytes_to_download == bytes_so_far,
        "didn't download full file"
    );
    while let Some(res) = download_tasks.join_next().await {
        res??;
    }

    tokio::task::spawn_blocking({
        let tmp_file = tmp_file.clone();
        move || {
            tmp_file.sync_all()?;
            Ok::<(), std::io::Error>(())
        }
    })
    .await??;

    let file_size = tokio::task::spawn_blocking({
        let tmp_file = tmp_file.clone();
        move || {
            let metadata = tmp_file.metadata()?;
            Ok::<_, std::io::Error>(metadata.len())
        }
    })
    .await??;
    ensure!(
        bytes_to_download == file_size,
        "didn't write full bytes to file"
    );

    let tmp_file_path =
        Arc::try_unwrap(tmp_file_path).expect("Multiple references to tmp_file_path");

    let out_path_clone = out_path.to_owned();
    tokio::task::spawn_blocking(move || {
        if existing_file_behavior == ExistingFileBehavior::Abort {
            ensure!(
                !out_path_clone.exists(),
                "{out_path_clone:?} already exists!"
            );
        }
        if existing_file_behavior == ExistingFileBehavior::Abort {
            tmp_file_path
                .persist_noclobber(&out_path_clone)
                .wrap_err("failed to persist temporary file")
        } else {
            tmp_file_path
                .persist(&out_path_clone)
                .wrap_err("failed to persist temporary file")
        }
    })
    .await
    .wrap_err("task panicked")??;

    Ok(())
}

async fn download_part_retry_on_timeout(
    range: &ContentRange,
    client: &Client,
    object: &S3Uri,
) -> Result<bytes::Bytes> {
    for _ in 0..PART_DOWNLOAD_NUM_RETRY {
        match timeout(
            Duration::from_secs(PART_DOWNLOAD_TIMEOUT_SECS), // Timeout for downloading one part
            download_part(range, client, object),
        )
        .await
        {
            Ok(result) => return result,
            Err(e) => warn!("get part timeout for part {}", e),
        }
    }

    Err(eyre!(
        "exceeded maximum number of retries for {object} at range {range}"
    ))
}

async fn download_part(
    range: &ContentRange,
    client: &Client,
    object: &S3Uri,
) -> Result<bytes::Bytes> {
    ensure!(
        !object.is_dir(),
        "directories are not supported, make sure the s3 uri doesn't end in a slash"
    );
    let part = client
        .get_object()
        .bucket(&object.bucket)
        .key(&object.key)
        .range(range.to_string())
        .send()
        .await
        .wrap_err("failed to make aws get_object request")?;

    let body = part
        .body
        .collect()
        .await
        .wrap_err("failed to collect body")?;

    Ok(body.into_bytes())
}
