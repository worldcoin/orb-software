use std::str::FromStr;

use camino::Utf8PathBuf;
use clap::Parser;
use color_eyre::{
    eyre::{bail, WrapErr},
    Result,
};
use orb_s3_helpers::{ExistingFileBehavior, S3Uri};
use thiserror::Error;
use tracing::{debug, info};

/// Boot the orb using NFS
#[derive(Debug, Parser)]
pub struct Nfsboot {
    /// The s3 URI of the RTS to use.
    #[arg(
        long,
        conflicts_with = "rts_path",
        required_unless_present = "rts_path"
    )]
    s3_url: Option<S3Uri>,
    /// The directory to save the s3 artifact we download.
    #[arg(long)]
    download_dir: Option<Utf8PathBuf>,
    /// Path to a downloaded RTS (zipped .tar or an already-extracted directory).
    #[arg(
        long,
        conflicts_with = "s3_url",
        conflicts_with = "download_dir",
        required_unless_present = "s3_url"
    )]
    rts_path: Option<Utf8PathBuf>,
    /// If this flag is given, overwites any existing files when downloading the rts.
    #[arg(long)]
    overwrite_existing: bool,
    /// Bind-mounts in the form <orb_mount_name>,<host_path>. Repeat --mount to add more.
    #[arg(long = "mount")]
    mounts: Vec<MountSpec>,
}

#[derive(Debug, Clone)]
#[cfg_attr(not(test), expect(dead_code))]
pub struct MountSpec {
    pub orb_mount_name: String,
    pub host_path: Utf8PathBuf,
}

#[derive(Debug, Error)]
pub enum MountSpecParseError {
    #[error("--mount expects <orb_mount_name>,<host_path>")]
    InvalidFormat,
}

impl FromStr for MountSpec {
    type Err = MountSpecParseError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let s = s.trim();
        if s.chars().any(char::is_whitespace) {
            return Err(MountSpecParseError::InvalidFormat);
        }

        let (left, right) = s
            .split_once(',')
            .ok_or(MountSpecParseError::InvalidFormat)?;

        if left.is_empty() || right.is_empty() {
            return Err(MountSpecParseError::InvalidFormat);
        }

        Ok(MountSpec {
            orb_mount_name: left.to_string(),
            host_path: Utf8PathBuf::from(right),
        })
    }
}

impl Nfsboot {
    pub async fn run(self) -> Result<()> {
        debug!("nfsboot called with args {self:?}");

        let existing_file_behavior = if self.overwrite_existing {
            ExistingFileBehavior::Overwrite
        } else {
            ExistingFileBehavior::Abort
        };

        // Determine RTS tarball path: download from S3 or use provided path
        let rts_path = if let Some(ref s3_url) = self.s3_url {
            assert!(
                self.rts_path.is_none(),
                "sanity: mutual exclusion guaranteed by clap"
            );
            let download_dir =
                self.download_dir.clone().unwrap_or_else(crate::current_dir);
            let download_path = download_dir.join(
                crate::download_s3::parse_filename(s3_url)
                    .wrap_err("failed to parse filename")?,
            );

            crate::download_s3::download_url(
                s3_url,
                &download_path,
                existing_file_behavior,
            )
            .await
            .wrap_err("error while downloading from s3")?;

            download_path
        } else if let Some(rts_path) = self.rts_path.clone() {
            assert!(
                self.s3_url.is_none(),
                "sanity: mutual exclusion guaranteed by clap"
            );
            assert!(
                self.download_dir.is_none(),
                "sanity: mutual exclusion guaranteed by clap"
            );
            info!("using already downloaded rts tarball");
            rts_path
        } else {
            bail!("you must provide either rts-path or s3-url");
        };

        debug!("resolved RTS path: {rts_path}");

        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mountspec_parses_valid() {
        let m: MountSpec = "data,/var/tmp".parse().expect("should parse");
        assert_eq!(m.orb_mount_name, "data");
        assert_eq!(m.host_path, Utf8PathBuf::from("/var/tmp"));
    }

    #[test]
    fn mountspec_rejects_missing_comma() {
        let m: Result<MountSpec, MountSpecParseError> = "foo".parse();
        assert!(matches!(m, Err(MountSpecParseError::InvalidFormat)));
    }

    #[test]
    fn mountspec_rejects_empty_left() {
        let m: Result<MountSpec, MountSpecParseError> = ",/path".parse();
        assert!(matches!(m, Err(MountSpecParseError::InvalidFormat)));
    }

    #[test]
    fn mountspec_rejects_empty_right() {
        let m: Result<MountSpec, MountSpecParseError> = "name,".parse();
        assert!(matches!(m, Err(MountSpecParseError::InvalidFormat)));
    }

    #[test]
    fn mountspec_rejects_space_after_comma() {
        let m: Result<MountSpec, MountSpecParseError> = "foo, bar".parse();
        assert!(matches!(m, Err(MountSpecParseError::InvalidFormat)));
    }

    #[test]
    fn mountspec_rejects_space_before_comma() {
        let m: Result<MountSpec, MountSpecParseError> = "foo ,bar".parse();
        assert!(matches!(m, Err(MountSpecParseError::InvalidFormat)));
    }
}
