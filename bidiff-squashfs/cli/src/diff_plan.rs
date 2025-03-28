//! Types involved in describing a [`DiffPlan`].

use std::{
    collections::{HashMap, HashSet},
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
use tracing::{debug, warn};

use crate::execute_plan::DiffPlanOutputs;

const SQFS_MAGIC: [u8; 4] = *b"hsqs";
const _: () = {
    // From https://dr-emann.github.io/squashfs/#superblock
    assert!(u32::from_le_bytes(SQFS_MAGIC) == 0x73717368);
};
const CLAIM_FILE: &str = "claim.json";

#[derive(Debug, Eq, PartialEq, Hash, Clone, derive_more::From)]
pub struct ComponentId(pub String);

impl From<&str> for ComponentId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

/// Holds information about different sources.
#[derive(Debug, Eq, PartialEq, Clone)]
struct SourceInfo {
    /// This is always relative to the OTA dir
    path: PathBuf,
    is_sqfs: bool,
    mime: MimeType,
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
    dir: PathBuf,
    /// The information extracted and validated about the different sources.
    sources: HashMap<ComponentId, SourceInfo>,
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
pub struct OutDir(PathBuf);

impl OutDir {
    pub fn new(dir: &Path) -> Self {
        Self(dir.to_path_buf())
    }
}

#[derive(Debug, Eq, PartialEq, Hash, Clone)]
pub enum Operation {
    Bidiff {
        id: ComponentId,
        old_path: PathBuf,
        new_path: PathBuf,
        out_path: PathBuf,
    },
    Copy {
        id: ComponentId,
        from_path: PathBuf,
        to_path: PathBuf,
    },
}

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct DiffPlan {
    pub ops: HashSet<Operation>,
}

impl DiffPlan {
    #[tracing::instrument(skip_all)]
    pub fn new(old: &OtaDir, new: &OtaDir, out_dir: &OutDir) -> Self {
        let changes = ComponentChanges::new(
            &old.sources.keys().map(ComponentId::to_owned).collect(),
            &new.sources.keys().map(ComponentId::to_owned).collect(),
        );
        debug!("component changes: {changes:?}");

        let diffed = detect_bidiffable(old, new);
        debug!("diffed: {diffed:?}");
        assert!(
            diffed.is_subset(&changes.kept),
            "sanity: only components that are kept could ever be diffable"
        );
        // Copied: (kept - diffed) + created
        let copied: HashSet<ComponentId> = changes
            .kept
            .difference(&diffed)
            .chain(&changes.created)
            .map(ToOwned::to_owned)
            .collect();
        debug!("copied: {copied:?}");
        assert!(
            copied.is_disjoint(&diffed),
            "sanity check: no diffed component should be copied"
        );
        assert!(
            copied.is_superset(&changes.created),
            "sanity check: any component that is created will also be copied"
        );

        let mut ops = HashSet::new();
        for id in copied {
            let from_source = new
                .sources
                .get(&id)
                .expect("infallible: `created` will exist in the new dir only");
            let from_path = new.dir.join(&from_source.path);
            let to_path = out_dir.0.join(&from_source.path);

            let op = Operation::Copy {
                id,
                from_path,
                to_path,
            };
            let inserted = ops.insert(op);
            assert!(
                inserted,
                "infallible: shouldn't be possible for us to create two equivalent operations"
            );
        }

        for id in diffed {
            let old_source = old
                .sources
                .get(&id)
                .expect("infallible: diffed components exist in both old and new");
            let new_source = new
                .sources
                .get(&id)
                .expect("infallible: diffed components exist in both old and new");

            let old_path = old.dir.join(&old_source.path);
            let new_path = new.dir.join(&new_source.path);
            let out_path = out_dir.0.join(&new_source.path);

            let op = Operation::Bidiff {
                id,
                old_path,
                new_path,
                out_path,
            };
            let inserted = ops.insert(op);
            assert!(
                inserted,
                "infallible: shouldn't be possible for us to create two equivalent operations"
            );
        }

        Self { ops }
    }
}

/// Newtype on [`UnckechedClaim`] for claims that have been patched by
/// [`patch_claim()`].
#[derive(Debug)]
pub struct PatchedClaim(#[expect(dead_code)] pub UncheckedClaim);

/// Patches the new/top OTA claim with the computed plan. Essentially this is
/// responsible for modifying the claim sources to account for any bidiff operations.
///
/// # Preconditions
/// `plan` was generated with `new_claim` coming from the newer `OtaDir`.
pub fn patch_claim(
    plan: &DiffPlan,
    plan_output: DiffPlanOutputs,
    new_claim: &UncheckedClaim,
) -> PatchedClaim {
    warn!("haven't implemented support for the hash and size parts of the claim patching yet");
    let mut output_claim = new_claim.clone();
    patch_claim_helper(plan, &mut output_claim.sources);

    PatchedClaim(output_claim)
}

/// Helper function that represents the bulk of the work of [`patch_claim`] but which is
/// a little bit more testable by virtue of not needing a fully formed
/// [`UncheckedClaim`].
fn patch_claim_helper(plan: &DiffPlan, sources: &mut HashMap<String, Source>) {
    assert_eq!(
        plan.ops.len(),
        sources.len(),
        "precondition failed: sources length didn't match plan length"
    );
    for op in plan.ops.iter() {
        let op_component_id = match op {
            Operation::Bidiff { id, .. } => id,
            Operation::Copy { id, .. } => id,
        };
        let source_val = sources.get(&op_component_id.0).expect(
            "precondition failed: sources names didnt match plan component ids",
        );
        assert_eq!(
            &op_component_id.0, &source_val.name,
            "sanity: map key should always match source name"
        );
    }

    let bidiffs = plan
        .ops
        .iter()
        .filter(|op| matches!(op, Operation::Bidiff { .. }));
    for op in bidiffs {
        let Operation::Bidiff {
            id,
            old_path: _,
            new_path,
            out_path,
        } = op
        else {
            unreachable!("we already filtered to only bidiffs");
        };
        let src = sources
            .get_mut(&id.0)
            .expect("infallible: bidiff components always exist in new_claim");
        src.mime_type = MimeType::ZstdBidiff;
        let LocalOrRemote::Local(ref mut output_component_path) = src.url else {
            panic!("precondition guarantees that all urls are local because this is a valid OtaDir.");
        };
        assert_eq!(
            output_component_path, new_path,
            "sanity: if the claim matches the plan this should be true"
        );
        output_component_path.clone_from(out_path);
    }
}

/// Helper struct using during [`DiffPlan`] to determine how component IDs between two
/// sets of components
#[derive(Debug)]
struct ComponentChanges {
    created: HashSet<ComponentId>,
    _deleted: HashSet<ComponentId>,
    kept: HashSet<ComponentId>,
}

impl ComponentChanges {
    pub fn new(old: &HashSet<ComponentId>, new: &HashSet<ComponentId>) -> Self {
        let deleted: HashSet<ComponentId> = old
            .iter()
            .filter(|id| !new.contains(*id))
            .map(|id| id.to_owned())
            .collect();

        let created: HashSet<ComponentId> = new
            .iter()
            .filter(|id| !old.contains(*id))
            .map(|id| id.to_owned())
            .collect();

        assert!(deleted.is_disjoint(&created), "sanity check");

        let kept: HashSet<ComponentId> = old
            .iter()
            .chain(new.iter())
            .filter(|id| !deleted.contains(id))
            .filter(|id| !created.contains(id))
            .map(|id| id.to_owned())
            .collect();

        assert!(kept.is_disjoint(&deleted), "sanity check");
        assert!(kept.is_disjoint(&created), "sanity check");

        Self {
            created,
            _deleted: deleted,
            kept,
        }
    }
}

fn detect_bidiffable(old: &OtaDir, new: &OtaDir) -> HashSet<ComponentId> {
    let filter_diffable =
        |sinfo: &SourceInfo| sinfo.mime == MimeType::OctetStream && sinfo.is_sqfs;
    let old_valid: HashSet<_> = old
        .sources
        .iter()
        .filter_map(|(id, sinfo)| filter_diffable(sinfo).then_some(id))
        .collect();

    let new_valid: HashSet<_> = new
        .sources
        .iter()
        .filter_map(|(id, sinfo)| filter_diffable(sinfo).then_some(id))
        .collect();

    old_valid
        .intersection(&new_valid)
        .map(|id| (*id).to_owned())
        .collect()
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

fn relative_path_from_source(source: &Source) -> Result<PathBuf> {
    let LocalOrRemote::Local(ref path) = source.url else {
        bail!("claim didn't match expected convention: all sources should be be of form `file://`");
    };
    ensure!(path.is_relative(), "source didn't match expected convention: all sources should be relative file paths");
    Ok(path.to_path_buf())
}

#[cfg(test)]
mod test_filter_valid_sqfs {
    // TODO(@thebutlah): write tests
}

#[cfg(test)]
mod test_diff_plan {
    use super::*;
    use test_log::test;

    fn helper(
        old_sources: impl IntoIterator<Item = (impl Into<String>, SourceInfo)>,
        new_sources: impl IntoIterator<Item = (impl Into<String>, SourceInfo)>,
    ) -> (OtaDir, OtaDir, OutDir) {
        // Arrange
        let old_ota_dir = Path::new("old");
        let old_ota = OtaDir {
            dir: old_ota_dir.to_path_buf(),
            sources: old_sources
                .into_iter()
                .map(|(id, sinfo)| (ComponentId(id.into()), sinfo))
                .collect(),
        };
        let new_ota_dir = Path::new("new");
        let new_ota = OtaDir {
            dir: new_ota_dir.to_path_buf(),
            sources: new_sources
                .into_iter()
                .map(|(id, sinfo)| (ComponentId(id.into()), sinfo))
                .collect(),
        };
        let out_dir = OutDir(PathBuf::from("out"));

        (old_ota, new_ota, out_dir)
    }

    #[test]
    fn test_no_components() {
        // Arrange
        let empty: HashMap<String, SourceInfo> = HashMap::new();
        let (old_ota, new_ota, out_dir) = helper(empty.clone(), empty);

        // Act
        let plan = DiffPlan::new(&old_ota, &new_ota, &out_dir);

        // Assert
        assert!(plan.ops.is_empty());
    }

    #[test]
    fn test_only_created() {
        // Arrange
        let empty: HashMap<String, _> = HashMap::new();
        let a_sinfo = SourceInfo {
            path: "a.cmp".into(),
            is_sqfs: false,
            mime: MimeType::OctetStream,
        };
        let created = HashMap::from([("a", a_sinfo.clone())]);
        let (old_ota, new_ota, out_dir) = helper(empty, created);

        // Act
        let plan = DiffPlan::new(&old_ota, &new_ota, &out_dir);

        // Assert
        assert_eq!(
            plan.ops,
            HashSet::from([Operation::Copy {
                id: "a".into(),
                from_path: new_ota.dir.join("a.cmp"),
                to_path: out_dir.0.join("a.cmp")
            }])
        );
    }

    #[test]
    fn test_only_deleted() {
        // Arrange
        let empty: HashMap<String, _> = HashMap::new();
        let a_sinfo = SourceInfo {
            path: "a.cmp".into(),
            is_sqfs: false,
            mime: MimeType::OctetStream,
        };
        let initial_sources = HashMap::from([("a", a_sinfo.clone())]);
        let (old_ota, new_ota, out_dir) = helper(initial_sources, empty);

        // Act
        let plan = DiffPlan::new(&old_ota, &new_ota, &out_dir);

        // Assert
        assert!(plan.ops.is_empty());
    }

    #[test]
    fn test_keep_1_not_sqfs_will_copy() {
        // Arrange
        let a_sinfo = SourceInfo {
            path: "a.cmp".into(),
            is_sqfs: false, // this is the important part
            mime: MimeType::OctetStream,
        };
        let sources = HashMap::from([("a", a_sinfo.clone())]);
        let (old_ota, new_ota, out_dir) = helper(sources.clone(), sources);

        // Act
        let plan = DiffPlan::new(&old_ota, &new_ota, &out_dir);

        // Assert
        assert_eq!(
            plan.ops,
            HashSet::from([Operation::Copy {
                id: "a".into(),
                from_path: new_ota.dir.join("a.cmp"),
                to_path: out_dir.0.join("a.cmp"),
            }])
        );
    }

    #[test]
    fn test_keep_1_is_sqfs_will_copy() {
        // Arrange
        let a_sinfo = SourceInfo {
            path: "a.cmp".into(),
            is_sqfs: true, // this is the important part
            mime: MimeType::OctetStream,
        };
        let sources = HashMap::from([("a", a_sinfo.clone())]);
        let (old_ota, new_ota, out_dir) = helper(sources.clone(), sources);

        // Act
        let plan = DiffPlan::new(&old_ota, &new_ota, &out_dir);

        // Assert
        assert_eq!(
            plan.ops,
            HashSet::from([Operation::Bidiff {
                id: "a".into(),
                old_path: old_ota.dir.join("a.cmp"),
                new_path: new_ota.dir.join("a.cmp"),
                out_path: out_dir.0.join("a.cmp"),
            }])
        );
    }

    #[test]
    fn test_keep_1_sqfs_and_1_not_sqfs() {
        // Arrange
        let sqfs_sinfo = SourceInfo {
            path: "sqfs.cmp".into(),
            is_sqfs: true,
            mime: MimeType::OctetStream,
        };
        let non_sqfs_sinfo = SourceInfo {
            path: "not-sqfs.cmp".into(),
            is_sqfs: false,
            mime: MimeType::OctetStream,
        };
        let sources = HashMap::from([
            ("sqfs", sqfs_sinfo.clone()),
            ("not-sqfs", non_sqfs_sinfo.clone()),
        ]);
        let (old_ota, new_ota, out_dir) = helper(sources.clone(), sources);

        // Act
        let plan = DiffPlan::new(&old_ota, &new_ota, &out_dir);

        // Assert
        assert_eq!(
            plan.ops,
            HashSet::from([
                Operation::Bidiff {
                    id: "sqfs".into(),
                    old_path: old_ota.dir.join("sqfs.cmp"),
                    new_path: new_ota.dir.join("sqfs.cmp"),
                    out_path: out_dir.0.join("sqfs.cmp"),
                },
                Operation::Copy {
                    id: "not-sqfs".into(),
                    from_path: new_ota.dir.join("not-sqfs.cmp"),
                    to_path: out_dir.0.join("not-sqfs.cmp"),
                }
            ])
        );
    }

    #[test]
    fn test_only_bidiff_sqfs_when_its_octet() {
        // Arrange
        let sqfs_octet_sinfo = SourceInfo {
            path: "sqfs-octet.cmp".into(),
            is_sqfs: true,
            mime: MimeType::OctetStream,
        };
        let sqfs_other_mime_sinfo = SourceInfo {
            path: "sqfs-other.cmp".into(),
            is_sqfs: true,
            mime: MimeType::XZ,
        };
        let sources = HashMap::from([
            ("sqfs-octet", sqfs_octet_sinfo.clone()),
            ("sqfs-other", sqfs_other_mime_sinfo.clone()),
        ]);
        let (old_ota, new_ota, out_dir) = helper(sources.clone(), sources);

        // Act
        let plan = DiffPlan::new(&old_ota, &new_ota, &out_dir);

        // Assert
        assert_eq!(
            plan.ops,
            HashSet::from([
                // Only the squashfs with OctetStream mime should be bidiffed
                Operation::Bidiff {
                    id: "sqfs-octet".into(),
                    old_path: old_ota.dir.join("sqfs-octet.cmp"),
                    new_path: new_ota.dir.join("sqfs-octet.cmp"),
                    out_path: out_dir.0.join("sqfs-octet.cmp"),
                },
                // The squashfs with other mime type should be copied
                Operation::Copy {
                    id: "sqfs-other".into(),
                    from_path: new_ota.dir.join("sqfs-other.cmp"),
                    to_path: out_dir.0.join("sqfs-other.cmp"),
                }
            ])
        );
    }

    #[test]
    fn test_complicated_setup() {
        // Arrange
        let kept_sqfs_sinfo = SourceInfo {
            path: "kept-sqfs.cmp".into(),
            is_sqfs: true,
            mime: MimeType::OctetStream,
        };
        let kept_non_sqfs_sinfo = SourceInfo {
            path: "kept-non-sqfs.cmp".into(),
            is_sqfs: false,
            mime: MimeType::OctetStream,
        };
        let deleted_sqfs_sinfo = SourceInfo {
            path: "deleted-sqfs.cmp".into(),
            is_sqfs: true,
            mime: MimeType::OctetStream,
        };
        let deleted_non_sqfs_sinfo = SourceInfo {
            path: "deleted-non-sqfs.cmp".into(),
            is_sqfs: false,
            mime: MimeType::OctetStream,
        };
        let created_sqfs_sinfo = SourceInfo {
            path: "created-sqfs.cmp".into(),
            is_sqfs: true,
            mime: MimeType::OctetStream,
        };
        let created_non_sqfs_sinfo = SourceInfo {
            path: "created-non-sqfs.cmp".into(),
            is_sqfs: false,
            mime: MimeType::OctetStream,
        };

        let old_sources = HashMap::from([
            ("kept-sqfs", kept_sqfs_sinfo.clone()),
            ("kept-non-sqfs", kept_non_sqfs_sinfo.clone()),
            ("deleted-sqfs", deleted_sqfs_sinfo),
            ("deleted-non-sqfs", deleted_non_sqfs_sinfo),
        ]);

        let new_sources = HashMap::from([
            ("kept-sqfs", kept_sqfs_sinfo),
            ("kept-non-sqfs", kept_non_sqfs_sinfo),
            ("created-sqfs", created_sqfs_sinfo),
            ("created-non-sqfs", created_non_sqfs_sinfo),
        ]);

        let (old_ota, new_ota, out_dir) = helper(old_sources, new_sources);

        // Act
        let plan = DiffPlan::new(&old_ota, &new_ota, &out_dir);

        // Assert
        assert_eq!(
            plan.ops,
            HashSet::from([
                // Kept components that are squashfs should be bidiffed
                Operation::Bidiff {
                    id: "kept-sqfs".into(),
                    old_path: old_ota.dir.join("kept-sqfs.cmp"),
                    new_path: new_ota.dir.join("kept-sqfs.cmp"),
                    out_path: out_dir.0.join("kept-sqfs.cmp"),
                },
                // Kept components that aren't squashfs should be copied
                Operation::Copy {
                    id: "kept-non-sqfs".into(),
                    from_path: new_ota.dir.join("kept-non-sqfs.cmp"),
                    to_path: out_dir.0.join("kept-non-sqfs.cmp"),
                },
                // Created components should be copied regardless of squashfs status
                Operation::Copy {
                    id: "created-sqfs".into(),
                    from_path: new_ota.dir.join("created-sqfs.cmp"),
                    to_path: out_dir.0.join("created-sqfs.cmp"),
                },
                Operation::Copy {
                    id: "created-non-sqfs".into(),
                    from_path: new_ota.dir.join("created-non-sqfs.cmp"),
                    to_path: out_dir.0.join("created-non-sqfs.cmp"),
                },
                // Deleted components should not appear in operations at all
            ])
        );
    }
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

#[cfg(test)]
mod test_component_changes {
    use super::*;

    use ComponentId as C;

    #[test]
    fn test_same_0() {
        let same = HashSet::new();
        let changes = ComponentChanges::new(&same, &same);

        assert!(changes._deleted.is_empty());
        assert!(changes.created.is_empty());
        assert_eq!(changes.kept, same);
    }

    #[test]
    fn test_same_1() {
        let same = HashSet::from(["a"]).into_iter().map(C::from).collect();
        let changes = ComponentChanges::new(&same, &same);

        assert!(changes._deleted.is_empty());
        assert!(changes.created.is_empty());
        assert_eq!(changes.kept, same);
    }

    #[test]
    fn test_same_2() {
        let same = HashSet::from(["a", "b"]).into_iter().map(C::from).collect();
        let changes = ComponentChanges::new(&same, &same);

        assert!(changes._deleted.is_empty());
        assert!(changes.created.is_empty());
        assert_eq!(changes.kept, same);
    }

    #[test]
    fn test_created_1() {
        let old: HashSet<ComponentId> = HashSet::new();
        let new = HashSet::from(["a"]).into_iter().map(C::from).collect();
        let changes = ComponentChanges::new(&old, &new);

        assert!(changes._deleted.is_empty());
        assert_eq!(changes.created, new);
        assert!(changes.kept.is_empty());
    }

    #[test]
    fn test_created_2() {
        let old: HashSet<ComponentId> = HashSet::new();
        let new = HashSet::from(["a", "b"]).into_iter().map(C::from).collect();
        let changes = ComponentChanges::new(&old, &new);

        assert!(changes._deleted.is_empty());
        assert_eq!(changes.created, new);
        assert!(changes.kept.is_empty());
    }

    #[test]
    fn test_deleted_1() {
        let old = HashSet::from(["a"]).into_iter().map(C::from).collect();
        let new: HashSet<ComponentId> = HashSet::new();
        let changes = ComponentChanges::new(&old, &new);

        assert_eq!(changes._deleted, old);
        assert!(changes.created.is_empty());
        assert!(changes.kept.is_empty());
    }

    #[test]
    fn test_deleted_2() {
        let old = HashSet::from(["a", "b"]).into_iter().map(C::from).collect();
        let new: HashSet<ComponentId> = HashSet::new();
        let changes = ComponentChanges::new(&old, &new);

        assert_eq!(changes._deleted, old);
        assert!(changes.created.is_empty());
        assert!(changes.kept.is_empty());
    }

    #[test]
    fn test_mixed() {
        let old = HashSet::from(["a", "b", "c"])
            .into_iter()
            .map(C::from)
            .collect();
        let new = HashSet::from(["b", "c", "d"])
            .into_iter()
            .map(C::from)
            .collect();
        let changes = ComponentChanges::new(&old, &new);

        assert_eq!(
            changes._deleted,
            HashSet::from(["a"]).into_iter().map(C::from).collect()
        );
        assert_eq!(
            changes.created,
            HashSet::from(["d"]).into_iter().map(C::from).collect()
        );
        assert_eq!(
            changes.kept,
            HashSet::from(["b", "c"]).into_iter().map(C::from).collect()
        );
    }
}

#[cfg(test)]
mod test_patch_claim {
    use super::*;
    use test_log::test;

    fn dummy_source(name: impl AsRef<str>, path: impl AsRef<Path>) -> Source {
        Source {
            name: name.as_ref().into(),
            url: LocalOrRemote::Local(path.as_ref().to_path_buf()),
            mime_type: MimeType::OctetStream,
            size: 0,
            hash: "".into(),
        }
    }

    #[test]
    fn test_no_ops_empty_sources() {
        // Arrange
        let empty_plan = DiffPlan {
            ops: HashSet::new(),
        };
        let mut empty_sources = HashMap::new();
        // Act
        patch_claim_helper(&empty_plan, &mut empty_sources);
        // Assert
        assert!(empty_sources.is_empty());
    }

    #[test]
    #[should_panic]
    fn test_no_ops_populated_sources_should_panic_due_to_precondition() {
        // Arrange
        let empty_plan = DiffPlan {
            ops: HashSet::new(),
        };
        let populated_sources =
            HashMap::from([("a".into(), dummy_source("a", "a.cmp"))]);
        let mut patched_sources = populated_sources.clone();
        // Act (should panic)
        patch_claim_helper(&empty_plan, &mut patched_sources);
    }

    #[test]
    #[should_panic]
    fn test_source_that_doesnt_appear_in_plan_panics_due_to_precondition() {
        // Arrange
        let a_to = PathBuf::from("a.to");
        let b_to = PathBuf::from("b.to");
        let plan = DiffPlan {
            ops: HashSet::from([Operation::Copy {
                id: "a".into(),
                from_path: "a.from".into(),
                to_path: a_to.clone(),
            }]),
        };
        let populated_sources = HashMap::from([
            ("a".into(), dummy_source("a", a_to)), // exists in plan
            ("b".into(), dummy_source("b", b_to)), // doesnt exist in plan
        ]);
        let mut patched_sources = populated_sources.clone();
        // Act (should panic)
        patch_claim_helper(&plan, &mut patched_sources);
    }

    #[test]
    fn test_only_copy_ops() {
        // Arrange
        let a_to = PathBuf::from("a.to");
        let b_to = PathBuf::from("b.to");
        let only_copy_ops = DiffPlan {
            ops: HashSet::from([
                Operation::Copy {
                    id: "a".into(),
                    from_path: "a.from".into(),
                    to_path: a_to.clone(),
                },
                Operation::Copy {
                    id: "b".into(),
                    from_path: "b.from".into(),
                    to_path: b_to.clone(),
                },
            ]),
        };
        let populated_sources = HashMap::from([
            ("a".into(), dummy_source("a", a_to)),
            ("b".into(), dummy_source("b", b_to)),
        ]);
        let mut patched_sources = populated_sources.clone();
        // Act
        patch_claim_helper(&only_copy_ops, &mut patched_sources);
        // Assert
        assert_eq!(
            patched_sources, populated_sources,
            "only had copy ops, so nothing should have changed"
        );
    }

    #[test]
    fn test_only_bidiff_ops() {
        // Arrange
        let a_new = PathBuf::from("a.new");
        let a_out = PathBuf::from("a.out");
        let b_new = PathBuf::from("b.new");
        let b_out = PathBuf::from("b.out");
        let only_bidiff_ops = DiffPlan {
            ops: HashSet::from([
                Operation::Bidiff {
                    id: "a".into(),
                    old_path: "a.old".into(),
                    new_path: a_new.clone(),
                    out_path: a_out.clone(),
                },
                Operation::Bidiff {
                    id: "b".into(),
                    old_path: "b.old".into(),
                    new_path: b_new.clone(),
                    out_path: b_out.clone(),
                },
            ]),
        };
        let original_sources = HashMap::from([
            ("a".into(), dummy_source("a", a_new)),
            ("b".into(), dummy_source("b", b_new)),
        ]);
        let mut patched_sources = original_sources.clone();

        // Act
        patch_claim_helper(&only_bidiff_ops, &mut patched_sources);

        // Assert
        assert_eq!(
            original_sources.len(),
            patched_sources.len(),
            "patched claim sources should have the same length as the original"
        );
        for (patched_source_name, patched_source) in patched_sources {
            assert_eq!(
                patched_source_name, patched_source.name,
                "sanity: source name matches key"
            );

            // Validate mime
            assert_eq!(
                patched_source.mime_type,
                MimeType::ZstdBidiff,
                "bidiff sources all have the application/zstd-bidff mime type"
            );

            // Validate name
            let original_source = original_sources
                .get(&patched_source_name)
                .expect("names should be unchanged from the original");

            // Validate URL
            {
                let LocalOrRemote::Local(ref original_path) = original_source.url
                else {
                    unreachable!("all our URLs are local");
                };
                assert_eq!(
                    original_path.extension().unwrap(),
                    "new",
                    "sanity: original url should end in .new"
                );
                let expected_url =
                    LocalOrRemote::Local(original_path.with_extension("out"));
                assert_eq!(
                    patched_source.url, expected_url,
                    "all urls should now end in .out"
                );
            }

            // Validate Hash
            {
                // TODO(ORBS-382): Handle hashes
            }

            // Validate Size
            {
                // TODO(ORBS_382): Handle sizes
            }
        }
    }
}
