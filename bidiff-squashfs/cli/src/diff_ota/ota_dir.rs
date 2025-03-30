use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    pin::pin,
};

use color_eyre::{
    eyre::{bail, ensure, WrapErr as _},
    Result,
};
use orb_update_agent_core::{LocalOrRemote, MimeType, Source, UncheckedClaim};
use tokio::{
    fs,
    io::{AsyncRead, AsyncReadExt as _, AsyncSeek, AsyncSeekExt as _},
};

use super::diff_plan::{ComponentId, CLAIM_FILE};

const SQFS_MAGIC: [u8; 4] = *b"hsqs";
const _: () = {
    // From https://dr-emann.github.io/squashfs/#superblock
    assert!(u32::from_le_bytes(SQFS_MAGIC) == 0x73717368);
};

/// Holds information about different sources.
#[derive(Debug, Eq, PartialEq, Clone)]
pub struct SourceInfo {
    /// This is always relative to the OTA dir
    pub path: PathBuf,
    pub is_sqfs: bool,
    pub mime: MimeType,
}

/// Represents a valid populated OTA directory, and information extracted from it.
///
/// This is an attempt to reduce the number of times we need to assert certain properties
/// about directories and instead document these requirements at the type system level.
/// It helps to consolidate all the complexity in one spot. It also helps to make
/// plan creation entirely sans-io.
///
/// # Invariants enforced
/// To construct this it must be a valid OTA directory. More concretely, the directory:
/// - must exist
/// - must be a directory
/// - must have a `claim.json` that deserializes
/// - all paths in the claim must point to files that actually exist.
/// - all paths in the claim must be local urls that are relative to the claim location.
// NOTE(@thebutlah): It was getting really confusing keeping track of filesystem state
// until making this change.
#[derive(Debug, Eq, PartialEq, Clone)]
pub struct OtaDir {
    pub dir: PathBuf,
    /// The information extracted and validated about the different sources.
    pub sources: HashMap<ComponentId, SourceInfo>,
}

impl OtaDir {
    /// Construct an `OtaDir` from a path.
    ///
    /// Inspects the directory and performs IO to extract relevant info.
    ///
    /// Returns the OtaDir and the deserialized [`UncheckedClaim`] that lives
    /// inside it.
    pub async fn new(dir: &Path) -> Result<(Self, UncheckedClaim)> {
        let claim_path = dir.join(CLAIM_FILE);
        let claim_contents = fs::read_to_string(&claim_path)
            .await
            .wrap_err_with(|| format!("missing claim at `{}`", claim_path.display()))?;
        let claim: UncheckedClaim = serde_json::from_str(&claim_contents)
            .wrap_err_with(|| {
                format!("failed to deserialize `{}` as claim", claim_path.display())
            })?;

        let mut sources = HashMap::new();
        for (id, source) in &claim.sources {
            let source_path_relative_to_ota_dir = relative_path_from_source(source)?;
            let source_path = dir.join(&source_path_relative_to_ota_dir);
            ensure!(
                fs::try_exists(&source_path).await.unwrap_or(false),
                "source {id} doesn't exist at `{source_path:?}`"
            );
            let source_file =
                tokio::fs::File::open(&source_path)
                    .await
                    .wrap_err_with(|| {
                        format!("failed to open component source at `{source_path:?}`")
                    })?;
            let is_sqfs = is_sqfs(source_file).await.wrap_err_with(|| {
                format!("failed to read component source at `{source_path:?}`")
            })?;
            let mime = source.mime_type.clone();
            let source_info = SourceInfo {
                path: source_path_relative_to_ota_dir,
                is_sqfs,
                mime,
            };
            sources.insert(ComponentId(id.to_owned()), source_info);
        }

        Ok((
            Self {
                dir: dir.to_path_buf(),
                sources,
            },
            claim,
        ))
    }
}

/// Newtype on a [`PathBuf`]. Represents a desired location on the filesystem that
/// the results of the final diffed OTA will be placed
#[derive(Debug, Eq, PartialEq, Clone, Hash)]
pub struct OutDir(pub PathBuf);

impl OutDir {
    pub fn new(dir: &Path) -> Self {
        Self(dir.to_path_buf())
    }
}

pub fn relative_path_from_source(source: &Source) -> Result<PathBuf> {
    let LocalOrRemote::Local(ref path) = source.url else {
        bail!("claim didn't match expected convention: all sources should be be of form `file://`");
    };
    ensure!(path.is_relative(), "source didn't match expected convention: all sources should be relative file paths");
    Ok(path.to_path_buf())
}

async fn is_sqfs(f: impl AsyncRead + AsyncSeek) -> Result<bool> {
    let mut f = pin!(f);
    f.rewind()
        .await
        .wrap_err("failed to seek to start of file")?;
    let magic = f
        .read_u32()
        .await
        .wrap_err("failed to read u32 from start of file")?;

    Ok(magic.to_be_bytes() == SQFS_MAGIC)
}

#[cfg(test)]
mod test_relative_path_from_source {
    use super::*;

    fn dummy_source(url: LocalOrRemote) -> Source {
        Source {
            name: "dummy".into(),
            url,
            mime_type: MimeType::OctetStream,
            size: 0,
            hash: "".into(),
        }
    }

    #[test]
    fn test_relative_local_url_success() {
        let path = PathBuf::from("components/foo.sqfs");
        let source = dummy_source(LocalOrRemote::Local(path.clone()));
        let result = relative_path_from_source(&source).unwrap();
        assert_eq!(result, path);
    }

    #[test]
    fn test_remote_url_fails() {
        let url =
            LocalOrRemote::Remote("https://example.com/foo.sqfs".parse().unwrap());
        let source = dummy_source(url);
        let result = relative_path_from_source(&source);
        assert!(result.is_err());
    }

    #[test]
    fn test_absolute_local_url_fails() {
        let path = PathBuf::from("/foo/bar/baz.sqfs");
        let source = dummy_source(LocalOrRemote::Local(path.clone()));
        let result = relative_path_from_source(&source);
        assert!(result.is_err());
    }
}

#[cfg(test)]
mod test_is_sqfs {
    use std::io::Cursor;

    use super::*;

    #[tokio::test]
    async fn test_example_squashfs() {
        // From https://dr-emann.github.io/squashfs/#superblock
        let data = 0x73717368u32.to_le_bytes();

        let data = Cursor::new(data);
        assert!(is_sqfs(data).await.unwrap());
    }

    #[tokio::test]
    async fn test_not_squashfs() {
        // Test some invalid magic numbers
        let data = Cursor::new([0x00, 0x00, 0x00, 0x00]);
        assert!(!is_sqfs(data).await.unwrap());

        let data = Cursor::new([0x7F, 0x45, 0x4C, 0x46]); // ELF magic number
        assert!(!is_sqfs(data).await.unwrap());

        let data = Cursor::new([0x1F, 0x8B, 0x08, 0x00]); // gzip magic number
        assert!(!is_sqfs(data).await.unwrap());
    }
}
