use std::{io::IsTerminal, str::FromStr, time::Duration};

use aws_config::{
    meta::{credentials::CredentialsProviderChain, region::RegionProviderChain},
    BehaviorVersion,
};
use aws_sdk_s3::config::ProvideCredentials;
use camino::Utf8Path;
use color_eyre::{
    eyre::{ensure, ContextCompat, OptionExt, WrapErr},
    Result, Section,
};
use indicatif::{ProgressState, ProgressStyle};
use tempfile::NamedTempFile;
use tracing::info;

#[derive(Debug, Eq, PartialEq)]
pub enum ExistingFileBehavior {
    /// If a file exists, overwrite it
    Overwrite,
    /// If a file exists, abort
    Abort,
}

/// `out_path` is the final path of the file after downloading.
pub async fn download_url(
    url: &str,
    out_path: &Utf8Path,
    existing_file_behavior: ExistingFileBehavior,
) -> Result<()> {
    if existing_file_behavior == ExistingFileBehavior::Abort {
        ensure!(!out_path.exists(), "{out_path:?} already exists!");
    }
    let parent_dir = out_path
        .parent()
        .expect("please provide the path to a file");
    ensure!(
        parent_dir.try_exists().unwrap_or(false),
        "parent directory {parent_dir} doesn't exist"
    );
    let s3_parts: S3UrlParts = url.parse().wrap_err("invalid s3 url")?;
    let (tmp_file, tmp_file_path) =
        tempfile::NamedTempFile::new_in(out_path.parent().unwrap())
            .wrap_err("failed to create tempfile")?
            .into_parts();
    let mut tmp_file: tokio::fs::File = tmp_file.into();

    let start_time = std::time::Instant::now();
    let resp = client()
        .await?
        .get_object()
        .bucket(s3_parts.bucket)
        .key(s3_parts.key)
        .send()
        .await
        .wrap_err("failed to make aws get_object request")?;
    let bytes_to_download = resp
        .content_length()
        .ok_or_eyre("expected a content length")?;

    let is_interactive = std::io::stdout().is_terminal();
    if is_interactive {
        info!("we are interactive");
    } else {
        info!("we are not interactive");
    }

    let bytes_to_download: u64 = bytes_to_download.try_into().expect("overflow");
    let pb = indicatif::ProgressBar::new(bytes_to_download);
    pb.set_style(ProgressStyle::with_template("{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({eta})")
        .unwrap()
        .with_key("eta", |state: &ProgressState, w: &mut dyn std::fmt::Write| write!(w, "{:.1}s", state.eta().as_secs_f64()).unwrap())
        .progress_chars("#>-"));

    let mut bytes_so_far = 0;
    let mut pct = 0;
    let reader =
        tokio_util::io::InspectReader::new(resp.body.into_async_read(), |bytes| {
            if !is_interactive {
                bytes_so_far += bytes.len() as u64;
                let new_pct = bytes_so_far * 100 / bytes_to_download;
                if new_pct > pct {
                    info!(
                        "Downloaded: ({}/{} MiB) {}%",
                        bytes_so_far >> 20,
                        bytes_to_download >> 20,
                        new_pct,
                    );
                }
                pct = new_pct;
            }
        });

    tokio::io::copy(&mut pb.wrap_async_read(reader), &mut tmp_file)
        .await
        .wrap_err("failed to download file")?;
    tmp_file
        .sync_all()
        .await
        .wrap_err("failed to finish saving file to disk")?;
    let file_size = tmp_file
        .metadata()
        .await
        .wrap_err("failed to inspect downloaded file size")?
        .len();
    assert_eq!(bytes_to_download, file_size);
    info!(
        "Downloaded {}MiB, took {}",
        bytes_to_download >> 20,
        elapsed_time_as_str(start_time.elapsed(),)
    );

    let tmp_file = NamedTempFile::from_parts(tmp_file.into_std().await, tmp_file_path);
    let out_path_clone = out_path.to_owned();
    tokio::task::spawn_blocking(move || {
        if existing_file_behavior == ExistingFileBehavior::Abort {
            ensure!(
                !out_path_clone.exists(),
                "{out_path_clone:?} already exists!"
            );
        }
        tmp_file
            .persist(out_path_clone)
            .wrap_err("failed to persist temporary file")
    })
    .await
    .wrap_err("task panicked")??;

    Ok(())
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
            "make sure that your aws credentials are set. Read more at \
            https://docs.aws.amazon.com/sdkref/latest/guide/file-format.html"
        })?;
    let config = aws_config::defaults(BehaviorVersion::v2024_03_28())
        .region(region_provider)
        .credentials_provider(credentials_provider)
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
