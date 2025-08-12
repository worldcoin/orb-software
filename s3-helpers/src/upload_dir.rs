//! Upload directory helper.

use std::path::Path;

use aws_sdk_s3::Client;
use aws_smithy_types::byte_stream::ByteStream;
use camino::Utf8Path;
use color_eyre::{
    eyre::{bail, WrapErr as _},
    Result,
};
use futures::{stream, StreamExt as _, TryStreamExt as _};

use crate::S3Uri;

/// Default number of concurrent uploads if none is specified.
const DEFAULT_CONCURRENCY: usize = 4;

/// Recursively uploads every file under `src_dir` to the destination prefix specified by
/// `dest_prefix`.
///
/// * `src_dir` – local directory whose **files** will be uploaded (sub‑directories are walked
///   recursively).
/// * `dest_prefix` – an `s3://bucket/prefix/` URI. **Must** end with `/` so that each file’s relative
///   path is simply concatenated onto the prefix.
/// * `concurrency` – optional limit for concurrent in‑flight `PutObject` requests (defaults to 4).
///
/// The final key for a file is `dest_prefix.key + relative_path` using `/` separators.
pub async fn upload_dir(
    client: &Client,
    src_dir: &Utf8Path,
    dest_prefix: &S3Uri,
    concurrency: Option<usize>,
) -> Result<()> {
    if !dest_prefix.is_dir() {
        bail!("dest_prefix must represent a directory and therefore end with '/' (got `{dest_prefix}`)");
    }

    let files = collect_files(src_dir.as_std_path()).await?;
    let concurrency = concurrency.unwrap_or(DEFAULT_CONCURRENCY).max(1);

    stream::iter(files)
        .map(|file_path| {
            let client = client.clone();
            let prefix = dest_prefix.clone();
            let src_root = src_dir.to_path_buf();
            async move {
                let rel_path = file_path
                    .strip_prefix(&src_root)
                    .expect("file inside dir");
                let rel_str = rel_path
                    .iter()
                    .map(|c| c.to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/");

                let key = format!("{}{}", prefix.key, rel_str);

                let body = ByteStream::from_path(&file_path)
                    .await
                    .wrap_err_with(|| format!("failed to open `{}`", file_path.display()))?;

                client
                    .put_object()
                    .bucket(&prefix.bucket)
                    .key(key.clone())
                    .body(body)
                    .send()
                    .await
                    .wrap_err_with(|| {
                        format!("failed to upload `{}` to s3://{}/{}", file_path.display(), prefix.bucket, key)
                    })?;

                Ok::<(), color_eyre::Report>(())
            }
        })
        .buffer_unordered(concurrency)
        .try_collect::<Vec<_>>()
        .await?;

    Ok(())
}

/// Recursively collects all file paths under `dir`.
async fn collect_files(dir: &Path) -> Result<Vec<std::path::PathBuf>> {
    let mut stack = vec![dir.to_path_buf()];
    let mut files = Vec::new();

    while let Some(cur) = stack.pop() {
        let mut rd = tokio::fs::read_dir(&cur)
            .await
            .wrap_err_with(|| format!("failed to read dir `{}`", cur.display()))?;
        while let Some(entry) = rd.next_entry().await? {
            let path = entry.path();
            let meta = entry.metadata().await?;
            if meta.is_dir() {
                stack.push(path);
            } else if meta.is_file() {
                files.push(path);
            }
        }
    }

    Ok(files)
}

// Integration tests for this module are located in `s3-helpers/tests/upload_dir.rs` so that they
// can share the LocalStack infrastructure with the other S3 helper tests.