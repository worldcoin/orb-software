use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    pin::pin,
};

use color_eyre::{
    eyre::{bail, ensure, WrapErr as _},
    Result,
};
use futures::FutureExt as _;
use orb_update_agent_core::{LocalOrRemote, MimeType, Source, UncheckedClaim};
use sha2::{Digest as _, Sha256};
use tokio::{
    fs,
    io::{AsyncRead, AsyncReadExt as _, AsyncSeek, AsyncSeekExt as _},
};

use super::{diff_plan::ComponentId, CLAIM_FILE};

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

        validate_sources(&claim.sources, dir)
            .await
            .wrap_err("failed to validate that claim matches sources")?;

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
    let Ok(magic) = f.read_u32().await else {
        // file is probably empty
        return Ok(false);
    };

    Ok(magic.to_be_bytes() == SQFS_MAGIC)
}

async fn validate_sources(
    sources: &HashMap<String, Source>,
    claim_dir: &Path,
) -> Result<()> {
    let mut hash_tasks = Vec::with_capacity(sources.len());
    for (source_id, source) in sources.iter() {
        ensure!(
            source_id == &source.name,
            "source id should match source name"
        );
        let LocalOrRemote::Local(ref path) = source.url else {
            bail!("claim contained a non-local path");
        };
        let component_path = claim_dir.join(path);

        let mdata = tokio::fs::metadata(&component_path)
            .await
            .wrap_err_with(|| {
                format!("failed to get metadata for source {source_id} at `{component_path:?}`")
            })?;
        ensure!(
            mdata.len() == source.size,
            "source {source_id} did not match claimed size"
        );

        let source_file = tokio::fs::File::open(&component_path)
            .await
            .wrap_err_with(|| {
                format!("failed to open source {source_id} at `{component_path:?}`")
            })?
            .into_std()
            .await;
        let source_id_cloned = source_id.to_owned();
        let hash_task = tokio::task::spawn_blocking(move || {
            hash_file(source_id_cloned, source_file)
        })
        .map(|r| r.wrap_err("hash task panicked")?);
        hash_tasks.push(hash_task);
    }

    let hash_task_results = futures::future::try_join_all(hash_tasks).await?;

    for (source_id, hash) in hash_task_results {
        let hash = hex::encode(hash);
        ensure!(
            hash == sources[&source_id].hash,
            "component {source_id} did not match claimed hash"
        );
    }

    Ok(())
}

fn hash_file(source_id: String, file: impl std::io::Read) -> Result<(String, Vec<u8>)> {
    let mut hasher = Sha256::new();
    let mut file = std::io::BufReader::new(file);
    std::io::copy(&mut file, &mut hasher)
        .wrap_err_with(|| format!("error while reading source `{source_id}`"))?;
    Ok((source_id, hasher.finalize().to_vec()))
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

        let data = Cursor::new([]); // Empty file
        assert!(!is_sqfs(data).await.unwrap());
    }
}
