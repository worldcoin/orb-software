#![allow(clippy::uninlined_format_args)]
use std::{
    io::IsTerminal as _,
    path::{Path, PathBuf},
};

use aws_sdk_s3::Client;
use color_eyre::{
    eyre::{OptionExt as _, WrapErr as _},
    Result,
};
use futures::TryStreamExt as _;
use orb_s3_helpers::{ClientExt as _, Progress, S3Uri};
use tracing::info;

use crate::ota_path::{OtaBucket, OtaPath};

// Precondition:
// - `client` must only ever be `None` when `path.is_local()`.
pub async fn fetch_path(
    client: Option<&Client>,
    path: &OtaPath,
    download_dir: &Path,
    ota_bucket: OtaBucket,
) -> Result<PathBuf> {
    if client.is_none() {
        assert!(
            path.is_local(),
            "client can only ever be `None` when the path is local"
        );
    }
    let s3_uri = match path {
        OtaPath::S3(s3_uri) => s3_uri.to_owned(),
        OtaPath::Version(ota_version) => ota_version.to_s3_uri(ota_bucket),
        OtaPath::Path(path_buf) => return Ok(path_buf.to_owned()),
    };

    let client = client.expect("infallible: nonlocal paths always have some client");

    let new_dir = download_dir.join(filename_from_s3(&s3_uri));
    tokio::fs::create_dir(&new_dir)
        .await
        .wrap_err_with(|| format!("failed to create dir at {new_dir:?}"))?;

    fetch_s3(client, &s3_uri, &new_dir)
        .await
        .wrap_err("failed to download {path}")?;

    Ok(new_dir)
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
    let pb = std::io::stderr().is_terminal().then(|| {
        indicatif::ProgressBar::new(total_ota_size)
            .with_style(crate::progress_bar_style())
    });

    let mut bytes_from_prev_objects = 0;
    for obj in objects {
        let key = obj
            .key
            .as_ref()
            .ok_or_eyre("encountered an object without a key")?;
        let key_suffix = key
            .strip_prefix(&s3_dir.key)
            .expect(
                "this object should always start with the same key prefix as `s3_dir`",
            )
            .to_owned();
        assert!(
            !key_suffix.starts_with('/'),
            "the prefix should always include a `/`",
        );
        let obj_uri = S3Uri {
            bucket: s3_dir.bucket.to_owned(),
            key: key.to_owned(),
        };
        let out_path = out_dir.join(&key_suffix);

        if let Some(ref pb) = pb {
            pb.set_message(key_suffix);
        } else {
            info!("downloading {obj_uri}");
        }

        let mut bytes_current_object = 0;
        let pb_callback = |progress: Progress| {
            bytes_current_object = progress.bytes_so_far;
            let bytes_so_far = bytes_from_prev_objects + bytes_current_object;
            if let Some(ref pb) = pb {
                pb.set_position(bytes_so_far);
            } else {
                let pct = (bytes_so_far * 100) / total_ota_size;
                if pct.is_multiple_of(5) {
                    info!(
                        "Downloaded: ({}/{} MiB) {}%",
                        bytes_so_far >> 20,
                        total_ota_size >> 20,
                        pct,
                    );
                }
            }
        };

        client
            .download_multipart(
                &obj_uri,
                out_path.as_path().try_into().unwrap(),
                orb_s3_helpers::ExistingFileBehavior::Abort,
                pb_callback,
            )
            .await
            .wrap_err_with(|| format!("failed to download {}", obj_uri))?;
        bytes_from_prev_objects += bytes_current_object;
    }
    if let Some(pb) = pb {
        pb.finish_and_clear();
    }

    Ok(())
}

fn filename_from_s3(ota: &S3Uri) -> String {
    ota.key.trim_end_matches('/').replace('/', "_")
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_no_nesting() {
        assert_eq!(filename_from_s3(&"s3://foo/bar/".parse().unwrap()), "bar");
        assert_eq!(filename_from_s3(&"s3://foo/bar".parse().unwrap()), "bar");
    }

    #[test]
    fn test_nesting() {
        assert_eq!(
            filename_from_s3(&"s3://foo/bar/baz/".parse().unwrap()),
            "bar_baz"
        );
        assert_eq!(
            filename_from_s3(&"s3://foo/bar/baz".parse().unwrap()),
            "bar_baz"
        );
    }
}
