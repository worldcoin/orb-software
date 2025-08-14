use std::str::FromStr;

use camino::Utf8PathBuf;
use clap::Parser;
use color_eyre::Result;
use orb_s3_helpers::S3Uri;
use thiserror::Error;
use tracing::debug;

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
    /// Path to a downloaded RTS (zipped .tar or an already-extracted directory).
    #[arg(long, conflicts_with = "s3_url", required_unless_present = "s3_url")]
    rts_path: Option<Utf8PathBuf>,
    /// Bind-mounts in the form <orb_mount_name>,<host_path>. Repeat --mount to add more.
    #[arg(long = "mount")]
    mounts: Vec<MountSpec>,
}

#[derive(Debug, Clone)]
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
        // Disallow any whitespace anywhere in the trimmed input
        if s.chars().any(char::is_whitespace) {
            return Err(MountSpecParseError::InvalidFormat);
        }

        let (left, right) = s
            .split_once(',')
            .ok_or(MountSpecParseError::InvalidFormat)?;

        // Disallow empty components
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
        todo!("nfsboot is not implemented yet");
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
