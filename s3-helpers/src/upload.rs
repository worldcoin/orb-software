#![allow(clippy::uninlined_format_args)]
use std::num::NonZeroU16;
use std::{collections::BTreeMap, os::unix::fs::FileExt, sync::Arc, time::Duration};

use aws_sdk_s3::types::{CompletedMultipartUpload, CompletedPart};
use aws_sdk_s3::Client;
use bytes::Bytes;
use camino::Utf8Path;
use color_eyre::{
    eyre::{bail, ensure, eyre, WrapErr as _},
    Result,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::{sync::Mutex, task::JoinSet, time::timeout};
use tracing::warn;

use crate::{ExistingObjectBehavior, S3Uri};

const PART_SIZE: u64 = 25 * 1024 * 1024; // 25 MiB
const CONCURRENCY: usize = 16;
const PART_UPLOAD_TIMEOUT_SECS: u64 = 120;
const PART_UPLOAD_NUM_RETRY: u8 = 5;

#[derive(Debug, Default)]
pub struct UploadProgress {
    pub bytes_so_far: u64,
    pub total_to_upload: u64,
}

#[derive(Debug, Clone, Copy)]
struct PartRange {
    start: u64,
    len: u64,
    part_number: NonZeroU16,
}

pub(crate) async fn upload_multipart(
    client: &Client,
    object: &S3Uri,
    in_path: &Utf8Path,
    existing_object_behavior: ExistingObjectBehavior,
    mut progress: impl FnMut(UploadProgress),
) -> Result<()> {
    ensure!(
        !object.is_dir(),
        "directories are not supported, make sure the s3 uri doesn't end in a slash"
    );

    let mut in_file = tokio::fs::File::open(in_path)
        .await
        .wrap_err_with(|| format!("failed to open {in_path}"))?;
    let metadata = in_file
        .metadata()
        .await
        .wrap_err_with(|| format!("failed to stat {in_path}"))?;
    ensure!(metadata.is_file(), "input path must be a file");
    let total_bytes = metadata.len();

    // Early check for object existence if we should abort.
    if matches!(existing_object_behavior, ExistingObjectBehavior::Abort) {
        let head_res = client
            .head_object()
            .bucket(&object.bucket)
            .key(&object.key)
            .send()
            .await;
        if head_res.is_ok() {
            bail!("object {object} already exists");
        }
    }

    // For small files, use a single PutObject.
    if total_bytes <= PART_SIZE {
        let mut bytes = Vec::new();
        in_file
            .read_to_end(&mut bytes)
            .await
            .wrap_err("failed to read input file")?;

        let mut builder = client
            .put_object()
            .bucket(&object.bucket)
            .key(&object.key)
            .body(Bytes::from(bytes).into());
        if matches!(existing_object_behavior, ExistingObjectBehavior::Abort) {
            builder = builder.if_none_match("*");
        }
        builder.send().await.wrap_err("failed to upload object")?;

        progress(UploadProgress {
            bytes_so_far: total_bytes,
            total_to_upload: total_bytes,
        });
        return Ok(());
    }

    let upload_id = client
        .create_multipart_upload()
        .bucket(&object.bucket)
        .key(&object.key)
        .send()
        .await
        .wrap_err("failed to create multipart upload")?
        .upload_id()
        .ok_or_else(|| eyre!("upload id missing"))?
        .to_string();

    in_file
        .flush()
        .await
        .wrap_err_with(|| format!("failed to flush {in_path}"))?;
    let file = Arc::new(in_file.try_into_std().expect("infallible"));

    let ranges_iter = {
        let mut part_number: u16 = 1;
        let step: usize = PART_SIZE.try_into().expect("PART_SIZE fits in usize");
        (0..total_bytes).step_by(step).map(move |start| {
            let len = std::cmp::min(PART_SIZE, total_bytes - start);
            let range = PartRange {
                start,
                len,
                part_number: part_number.try_into().unwrap(),
            };
            part_number += 1;
            range
        })
    };

    let ranges = Arc::new(Mutex::new(ranges_iter));
    let mut tasks = JoinSet::new();

    let (progress_tx, mut progress_rx) = tokio::sync::mpsc::unbounded_channel();
    for _ in 0..CONCURRENCY {
        let client = client.clone();
        let object = object.clone();
        let upload_id = upload_id.clone();
        let file = Arc::clone(&file);
        let ranges = Arc::clone(&ranges);
        let progress_tx = progress_tx.clone();

        tasks.spawn(async move {
            loop {
                let range_opt = {
                    let mut lock = ranges.lock().await;
                    lock.next()
                };
                let Some(range) = range_opt else { break };

                let buf = tokio::task::spawn_blocking({
                    let file = Arc::clone(&file);
                    move || {
                        let mut buf = vec![0u8; range.len as usize];
                        file.read_exact_at(&mut buf, range.start)?;
                        Ok::<_, std::io::Error>((range, buf))
                    }
                })
                .await??;

                let (range, buf) = buf;
                let etag = upload_part_retry_on_timeout(
                    &client,
                    &object,
                    &upload_id,
                    range.part_number,
                    Bytes::from(buf),
                )
                .await?;

                if progress_tx
                    .send((range.len, range.part_number, etag))
                    .is_err()
                {
                    break;
                }
            }

            Ok::<(), color_eyre::Report>(())
        });
    }
    drop(progress_tx);

    let mut bytes_so_far: u64 = 0;
    let mut parts: BTreeMap<NonZeroU16, String> = BTreeMap::new();
    while let Some((len, part_number, etag)) = progress_rx.recv().await {
        bytes_so_far += len;
        parts.insert(part_number, etag);
        progress(UploadProgress {
            bytes_so_far,
            total_to_upload: total_bytes,
        });
    }

    let mut task_err: Option<color_eyre::Report> = None;
    while let Some(res) = tasks.join_next().await {
        if let Err(e) | Ok(Err(e)) = res.map_err(Into::into) {
            task_err = Some(e);
            break;
        }
    }

    if let Some(e) = task_err {
        // Try to abort the multipart upload on failure
        let _ = client
            .abort_multipart_upload()
            .bucket(&object.bucket)
            .key(&object.key)
            .upload_id(&upload_id)
            .send()
            .await;
        return Err(e);
    }

    // Build parts in order
    let completed_parts = parts
        .into_iter()
        .map(|(part_number, e_tag)| {
            CompletedPart::builder()
                .e_tag(e_tag)
                .part_number(part_number.get().into())
                .build()
        })
        .collect::<Vec<_>>();

    client
        .complete_multipart_upload()
        .bucket(&object.bucket)
        .key(&object.key)
        .upload_id(upload_id)
        .multipart_upload(
            CompletedMultipartUpload::builder()
                .set_parts(Some(completed_parts))
                .build(),
        )
        .send()
        .await
        .wrap_err("failed to complete multipart upload")?;

    Ok(())
}

async fn upload_part_retry_on_timeout(
    client: &Client,
    object: &S3Uri,
    upload_id: &str,
    part_number: NonZeroU16,
    body: Bytes,
) -> Result<String> {
    for _ in 0..PART_UPLOAD_NUM_RETRY {
        match timeout(
            Duration::from_secs(PART_UPLOAD_TIMEOUT_SECS),
            upload_part(client, object, upload_id, part_number, body.clone()),
        )
        .await
        {
            Ok(result) => return result,
            Err(e) => warn!("put part timeout for part {}: {}", part_number, e),
        }
    }

    Err(eyre!(
        "exceeded maximum number of retries for {object} part {part_number}"
    ))
}

async fn upload_part(
    client: &Client,
    object: &S3Uri,
    upload_id: &str,
    part_number: NonZeroU16,
    body: Bytes,
) -> Result<String> {
    let resp = client
        .upload_part()
        .bucket(&object.bucket)
        .key(&object.key)
        .upload_id(upload_id)
        .part_number(u16::from(part_number).into())
        .body(body.into())
        .send()
        .await
        .wrap_err("failed to upload part")?;

    let etag = resp
        .e_tag()
        .ok_or_else(|| eyre!("etag missing in upload_part response"))?
        .to_string();
    Ok(etag)
}
