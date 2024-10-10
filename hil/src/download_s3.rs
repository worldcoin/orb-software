use std::{
    fs::File,
    io::IsTerminal,
    os::unix::fs::FileExt,
    str::FromStr,
    sync::atomic::{AtomicU64, Ordering},
    sync::Arc,
    time::Duration,
};

use aws_config::{
    meta::{credentials::CredentialsProviderChain, region::RegionProviderChain},
    retry::RetryConfig,
    stalled_stream_protection::StalledStreamProtectionConfig,
    BehaviorVersion,
};
use aws_sdk_s3::config::ProvideCredentials;
use aws_sdk_s3::Client;
use camino::Utf8Path;
use color_eyre::{
    eyre::{ensure, ContextCompat, OptionExt, WrapErr},
    Result, Section,
};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio::time::timeout;
use tracing::{info, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExistingFileBehavior {
    /// If a file exists, overwrite it
    Overwrite,
    /// If a file exists, abort
    Abort,
}

#[derive(Debug)]
struct ContentRange {
    start: u64,
    end: u64,
}

impl ContentRange {
    fn new(start: u64, end: u64) -> Self {
        Self { start, end }
    }
}

impl std::fmt::Display for ContentRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "bytes={}-{}", self.start, self.end)
    }
}

pub async fn download_url(
    url: &str,
    out_path: &Utf8Path,
    existing_file_behavior: ExistingFileBehavior,
) -> Result<(), color_eyre::Report> {
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
    let s3_parts: S3UrlParts = url.parse().wrap_err("invalid s3 url")?;

    let start_time = std::time::Instant::now();
    let client = client().await?;
    let head_resp = client
        .head_object()
        .bucket(&s3_parts.bucket)
        .key(&s3_parts.key)
        .send()
        .await
        .wrap_err("failed to make aws head_object request")?;

    let bytes_to_download = head_resp.content_length().unwrap();

    let bytes_to_download: u64 = bytes_to_download.try_into().expect("overflow");

    let is_interactive = std::io::stdout().is_terminal();
    let pb = indicatif::ProgressBar::new(bytes_to_download);
    pb.set_style(
        indicatif::ProgressStyle::with_template(
            "{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({eta})",
        )
        .unwrap()
        .with_key("eta", |state: &indicatif::ProgressState, w: &mut dyn std::fmt::Write| {
            write!(w, "{:.1}s", state.eta().as_secs_f64()).unwrap()
        })
        .progress_chars("#>-"),
    );

    let part_size = 25 * 1024 * 1024; // Part size can be chosen at will, set to 25 MiB
    let num_parts = (bytes_to_download + part_size - 1) / part_size;

    let concurrency = 8;
    let semaphore = Arc::new(Semaphore::new(concurrency));

    let (tmp_file, tmp_file_path) = tokio::task::spawn_blocking(move || {
        let tmp_file = tempfile::NamedTempFile::new_in(parent_dir)
            .wrap_err("failed to create tempfile")?;
        tmp_file.as_file().set_len(bytes_to_download)?;
        Ok::<_, color_eyre::Report>(tmp_file.into_parts())
    })
    .await?
    .wrap_err("failed to create tempfile")?;

    let tmp_file: Arc<File> = Arc::new(tmp_file);
    let bytes_downloaded = Arc::new(AtomicU64::new(0));
    let out_path_clone = out_path.to_owned();
    let tmp_file_path = Arc::new(tmp_file_path);
    let mut tasks = JoinSet::new();

    for part_number in 0..num_parts {
        let semaphore = semaphore.clone();
        let client = client.clone();
        let bucket = s3_parts.bucket.clone();
        let key = s3_parts.key.clone();
        let tmp_file = tmp_file.clone();
        let pb = pb.clone();
        let bytes_downloaded = bytes_downloaded.clone();

        let start = part_number * part_size;
        let end = std::cmp::min(start + part_size, bytes_to_download) - 1;
        let range = ContentRange::new(start, end);

        tasks.spawn(async move {
            let _permit = semaphore.acquire().await;

            let body = download_part_retry_on_timeout(
                part_number,
                &range,
                &client,
                &bucket,
                &key,
            )
            .await?;

            let chunk_size = body.len() as u64;

            tokio::task::spawn_blocking(move || {
                tmp_file.write_all_at(&body, start)?;
                Ok::<(), std::io::Error>(())
            })
            .await??;

            if is_interactive {
                pb.inc(chunk_size);
            } else {
                let bytes_so_far = bytes_downloaded
                    .fetch_add(chunk_size, Ordering::Relaxed)
                    + chunk_size;
                let pct = (bytes_so_far * 100) / bytes_to_download;
                if pct % 5 == 0 {
                    info!(
                        "Downloaded: ({}/{} MiB) {}%",
                        bytes_so_far >> 20,
                        bytes_to_download >> 20,
                        pct,
                    );
                }
            }

            Ok::<(), color_eyre::Report>(())
        });
    }

    while let Some(res) = tasks.join_next().await {
        res??;
    }

    pb.finish_and_clear();

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
    assert_eq!(bytes_to_download, file_size);

    info!(
        "Downloaded {}MiB, took {}",
        bytes_to_download >> 20,
        elapsed_time_as_str(start_time.elapsed(),)
    );

    let tmp_file_path =
        Arc::try_unwrap(tmp_file_path).expect("Multiple references to tmp_file_path");

    tokio::task::spawn_blocking(move || {
        if existing_file_behavior == ExistingFileBehavior::Abort {
            ensure!(
                !out_path_clone.exists(),
                "{out_path_clone:?} already exists!"
            );
        }
        tmp_file_path
            .persist_noclobber(&out_path_clone)
            .wrap_err("failed to persist temporary file")
    })
    .await
    .wrap_err("task panicked")??;

    Ok(())
}

async fn download_part_retry_on_timeout(
    id: u64,
    range: &ContentRange,
    client: &Client,
    bucket: &str,
    key: &str,
) -> Result<bytes::Bytes, color_eyre::Report> {
    loop {
        match timeout(
            Duration::from_secs(30), // Timeout for downloading one part
            download_part(range, client, bucket, key),
        )
        .await
        {
            Ok(result) => return result,
            Err(e) => warn!("get part timeout for part {}: {}", id, e),
        }
    }
}

async fn download_part(
    range: &ContentRange,
    client: &Client,
    bucket: &str,
    key: &str,
) -> Result<bytes::Bytes, color_eyre::Report> {
    let part = client
        .get_object()
        .bucket(bucket)
        .key(key)
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

async fn client() -> Result<aws_sdk_s3::Client> {
    let region_provider = RegionProviderChain::default_provider().or_else("us-east-1");
    let region = region_provider.region().await.expect("infallible");
    info!("using aws region: {region}");
    let credentials_provider = CredentialsProviderChain::default_provider().await;
    let _creds = credentials_provider
        .provide_credentials()
        .await
        .wrap_err("failed to get aws credentials")
        .with_note(|| {
            format!("AWS_PROFILE env var was {:?}", std::env::var("AWS_PROFILE"))
        })
        .with_suggestion(|| {
            "make sure that your aws credentials are set. Follow the instructions at
            https://worldcoin.github.io/orb-software/hil/cli."
        })
        .with_suggestion(|| "try running `AWS_PROFILE=hil aws sso login`")?;

    let retry_config = RetryConfig::standard().with_max_attempts(5);

    let config = aws_config::defaults(BehaviorVersion::v2024_03_28())
        .region(region_provider)
        .credentials_provider(credentials_provider)
        .retry_config(retry_config)
        .stalled_stream_protection(StalledStreamProtectionConfig::disabled())
        .load()
        .await;

    Ok(aws_sdk_s3::Client::new(&config))
}

#[derive(Debug, Eq, PartialEq)]
struct S3UrlParts {
    bucket: String,
    key: String,
}

impl FromStr for S3UrlParts {
    type Err = color_eyre::Report;

    fn from_str(s: &str) -> std::prelude::v1::Result<Self, Self::Err> {
        let (bucket, key) = s
            .strip_prefix("s3://")
            .ok_or_eyre("must be a url that starts with `s3://`")?
            .split_once('/')
            .ok_or_eyre("expected s3://<bucket>/<key>")?;
        Ok(Self {
            bucket: bucket.to_owned(),
            key: key.to_owned(),
        })
    }
}

/// Calculates the filename based on the s3 url.
pub fn parse_filename(url: &str) -> Result<String> {
    let expected_prefix = "s3://worldcoin-orb-update-packages-stage/worldcoin/orb-os/";
    let path = url
        .strip_prefix(expected_prefix)
        .wrap_err_with(|| format!("missing url prefix of {expected_prefix}"))?;
    let splits: Vec<_> = path.split('/').collect();
    ensure!(
        splits.len() == 3,
        "invalid number of '/' delineated segments in the url"
    );
    ensure!(
        splits[2].contains(".tar."),
        "it doesn't look like this url ends in a tarball"
    );
    Ok(format!("{}-{}", splits[0], splits[2]))
}

fn elapsed_time_as_str(time: Duration) -> String {
    let total_secs = time.as_secs();
    let minutes = total_secs / 60;
    let remaining_secs = total_secs % 60;
    format!("{minutes}m{remaining_secs}s")
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn test_elapsed_time_as_str() {
        assert_eq!("0m0s", elapsed_time_as_str(Duration::ZERO));
        assert_eq!("0m0s", elapsed_time_as_str(Duration::from_millis(999)));
        assert_eq!("0m1s", elapsed_time_as_str(Duration::from_millis(1000)));
        assert_eq!("0m1s", elapsed_time_as_str(Duration::from_millis(1001)));

        assert_eq!("0m59s", elapsed_time_as_str(Duration::from_secs(59)));
        assert_eq!("1m0s", elapsed_time_as_str(Duration::from_secs(60)));
        assert_eq!("1m1s", elapsed_time_as_str(Duration::from_secs(61)));

        assert_eq!(
            "61m59s",
            elapsed_time_as_str(Duration::from_secs(61 * 60 + 59))
        );
    }

    #[test]
    fn test_parse() -> color_eyre::Result<()> {
        let examples = [
            (
                "s3://worldcoin-orb-update-packages-stage/worldcoin/orb-os/2024-05-07-heads-main-0-g4b8aae5/rts/rts-dev.tar.zst",
                "2024-05-07-heads-main-0-g4b8aae5-rts-dev.tar.zst"
            ),
            (
                "s3://worldcoin-orb-update-packages-stage/worldcoin/orb-os/2024-05-08-remotes-pull-386-merge-0-geea20f1/rts/rts-prod.tar.zst",
                "2024-05-08-remotes-pull-386-merge-0-geea20f1-rts-prod.tar.zst"
            ),
            (
                "s3://worldcoin-orb-update-packages-stage/worldcoin/orb-os/2024-05-08-tags-release-5.0.39-0-ga12b3d7/rts/rts-dev.tar.zst",
                "2024-05-08-tags-release-5.0.39-0-ga12b3d7-rts-dev.tar.zst"
            ),
        ];
        for (url, expected_filename) in examples {
            assert_eq!(parse_filename(url)?, expected_filename);
        }
        Ok(())
    }
}
