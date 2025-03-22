use std::path::{Path, PathBuf};

use aws_sdk_s3::Client;
use color_eyre::{eyre::WrapErr as _, Result};
use futures::TryStreamExt as _;
use orb_s3_helpers::{ClientExt as _, S3Uri};

use crate::ota_path::{OtaPath, OtaVersion};

pub async fn fetch_path(client: &Client, path: &OtaPath) -> Result<PathBuf> {
    let download_dir = tempfile::tempdir_in(".")
        .wrap_err("failed to create temporary directory in current directoyr")?;

    match path {
        OtaPath::S3(s3_uri) => fetch_s3(client, s3_uri, download_dir.path()).await,
        OtaPath::Version(ota_version) => {
            fetch_s3(
                client,
                &s3_from_ota_version(ota_version),
                download_dir.path(),
            )
            .await
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
    let mut stream = client.list_prefix(s3_dir);
    while let Some(obj) = stream
        .try_next()
        .await
        .wrap_err("error while listing s3 dir")?
    {
        client.get_object().

    }
    todo!()
}

fn s3_from_ota_version(ota: &OtaVersion) -> S3Uri {
    format!("s3://worldcoin-orb-updates-stage/{}/", ota.as_str())
        .parse()
        .expect("this should always parse")
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_ota_to_s3() {
        let ota: OtaVersion = "ota://1.2.3".parse().expect("valid ota");
        let s3: S3Uri = "s3://worldcoin-orb-updates-stage/1.2.3/"
            .parse()
            .expect("valid s3");
        let converted = s3_from_ota_version(&ota);
        assert_eq!(converted, s3);
        assert!(converted.is_dir())
    }

    #[test]
    fn test_ota_to_s3_real_example() {
        let ota: OtaVersion = "ota://6.0.29+5d20de6.2410071904.dev"
            .parse()
            .expect("valid ota");
        let s3: S3Uri =
            "s3://worldcoin-orb-updates-stage/6.0.29+5d20de6.2410071904.dev/"
                .parse()
                .expect("valid s3");
        let converted = s3_from_ota_version(&ota);
        assert_eq!(converted, s3);
        assert!(converted.is_dir())
    }
}
