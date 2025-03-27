//! Types involved in describing a [`DiffPlan`].

use std::{
    collections::{HashMap, HashSet},
    path::Path,
    pin::pin,
};

use color_eyre::{
    eyre::{bail, ensure, WrapErr as _},
    Result,
};
use orb_update_agent_core::{LocalOrRemote, Source, UncheckedClaim};
use tokio::{
    fs,
    io::{AsyncRead, AsyncReadExt as _, AsyncSeek, AsyncSeekExt as _},
};

const SQFS_MAGIC: [u8; 4] = *b"hsqs";
const _: () = {
    // From https://dr-emann.github.io/squashfs/#superblock
    assert!(u32::from_le_bytes(SQFS_MAGIC) == 0x73717368);
};

#[derive(Debug, Eq, PartialEq, Hash, Clone, derive_more::From)]
pub struct ComponentId(pub String);

impl From<&str> for ComponentId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

#[derive(Debug, Eq, PartialEq, Hash, Clone)]
pub enum Operation {
    Bidiff(ComponentId),
    Copy(ComponentId),
    Delete(ComponentId),
    Create(ComponentId),
}

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct DiffPlan(HashSet<Operation>);

impl DiffPlan {
    pub async fn new(old_claim_path: &Path, new_claim_path: &Path) -> Result<Self> {
        let old_claim =
            fs::read_to_string(&old_claim_path)
                .await
                .wrap_err_with(|| {
                    format!("missing claim at `{}`", old_claim_path.display())
                })?;
        let old_claim: UncheckedClaim = serde_json::from_str(&old_claim)
            .wrap_err_with(|| {
                format!(
                    "failed to deserialize `{}` as claim",
                    old_claim_path.display()
                )
            })?;
        let new_claim =
            fs::read_to_string(&new_claim_path)
                .await
                .wrap_err_with(|| {
                    format!("missing claim at `{}`", new_claim_path.display())
                })?;
        let new_claim: UncheckedClaim = serde_json::from_str(&new_claim)
            .wrap_err_with(|| {
                format!(
                    "failed to deserialize `{}` as claim",
                    new_claim_path.display()
                )
            })?;

        let changes = ComponentChanges::new(
            &old_claim
                .sources
                .keys()
                .map(|id| ComponentId(id.to_owned()))
                .collect(),
            &new_claim
                .sources
                .keys()
                .map(|id| ComponentId(id.to_owned()))
                .collect(),
        );

        let diffable = detect_bidiffable(
            &old_claim.sources,
            old_claim_path,
            &new_claim.sources,
            new_claim_path,
        )
        .await
        .wrap_err("error while detecting diffable sources")?;
        assert!(
            diffable.is_subset(&changes.kept),
            "sanity: only components that are kept could ever be diffable"
        );
        let copied: HashSet<ComponentId> = changes
            .kept
            .difference(&diffable)
            .map(ToOwned::to_owned)
            .collect();
        assert!(
            copied.is_disjoint(&diffable),
            "sanity check: any kept component that we dont diff, we copy"
        );

        let mut result = HashSet::new();
        result.extend(changes.created.into_iter().map(Operation::Create));
        result.extend(changes.deleted.into_iter().map(Operation::Delete));
        result.extend(copied.into_iter().map(Operation::Copy));
        result.extend(diffable.into_iter().map(Operation::Bidiff));

        Ok(Self(result))
    }
}

#[derive(Debug)]
struct ComponentChanges {
    created: HashSet<ComponentId>,
    deleted: HashSet<ComponentId>,
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
            deleted,
            kept,
        }
    }
}

async fn detect_bidiffable(
    old: &HashMap<String, Source>,
    old_claim_path: &Path,
    new: &HashMap<String, Source>,
    new_claim_path: &Path,
) -> Result<HashSet<ComponentId>> {
    let old_valid = filter_valid_sqfs(old_claim_path, old.iter())
        .await
        .wrap_err("failed to filter for squashfs in old claim")?;
    let new_valid = filter_valid_sqfs(new_claim_path, new.iter())
        .await
        .wrap_err("failed to filter for squashfs in new claim")?;

    Ok(old_valid
        .intersection(&new_valid)
        .map(|id| id.to_owned())
        .collect())
}

async fn filter_valid_sqfs(
    claim_path: &Path,
    sources: impl Iterator<Item = (impl AsRef<str>, &Source)>,
) -> Result<HashSet<ComponentId>> {
    ensure!(
        fs::metadata(claim_path).await?.is_file(),
        "expected `{claim_path:?}` to be a file"
    );
    let base_path = claim_path
        .parent()
        .expect("infallible, we already checked its a file");
    let extract_path = |source: &Source| {
        let LocalOrRemote::Local(ref path) = source.url else {
            bail!("claim didn't match expected convention: all sources should be be of form `file://`");
        };
        ensure!(path.is_relative(), "claim didn't match expected convention: all sources should be relative file paths");

        Ok(base_path.join(path))
    };

    let mut sqfs_components = HashSet::new();
    for (id, source) in sources
        // We assume sqfs are always OctetStream
        .filter(|(_id, s)| s.mime_type == orb_update_agent_core::MimeType::OctetStream)
    {
        let path = extract_path(source).wrap_err_with(|| {
            format!("failed to determine path for source {}", source.name)
        })?;
        let file = tokio::fs::File::open(&path).await.wrap_err_with(|| {
            format!("failed to open component source at `{path:?}`")
        })?;
        if !is_sqfs(file).await? {
            continue;
        }

        let id = ComponentId::from(id.as_ref());
        sqfs_components.insert(id);
    }

    Ok(sqfs_components)
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
mod test_filter_valid_sqfs {
    // TODO(@thebutlah): write tests
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

        assert!(changes.deleted.is_empty());
        assert!(changes.created.is_empty());
        assert_eq!(changes.kept, same);
    }

    #[test]
    fn test_same_1() {
        let same = HashSet::from(["a"]).into_iter().map(C::from).collect();
        let changes = ComponentChanges::new(&same, &same);

        assert!(changes.deleted.is_empty());
        assert!(changes.created.is_empty());
        assert_eq!(changes.kept, same);
    }

    #[test]
    fn test_same_2() {
        let same = HashSet::from(["a", "b"]).into_iter().map(C::from).collect();
        let changes = ComponentChanges::new(&same, &same);

        assert!(changes.deleted.is_empty());
        assert!(changes.created.is_empty());
        assert_eq!(changes.kept, same);
    }

    #[test]
    fn test_created_1() {
        let old: HashSet<ComponentId> = HashSet::new();
        let new = HashSet::from(["a"]).into_iter().map(C::from).collect();
        let changes = ComponentChanges::new(&old, &new);

        assert!(changes.deleted.is_empty());
        assert_eq!(changes.created, new);
        assert!(changes.kept.is_empty());
    }

    #[test]
    fn test_created_2() {
        let old: HashSet<ComponentId> = HashSet::new();
        let new = HashSet::from(["a", "b"]).into_iter().map(C::from).collect();
        let changes = ComponentChanges::new(&old, &new);

        assert!(changes.deleted.is_empty());
        assert_eq!(changes.created, new);
        assert!(changes.kept.is_empty());
    }

    #[test]
    fn test_deleted_1() {
        let old = HashSet::from(["a"]).into_iter().map(C::from).collect();
        let new: HashSet<ComponentId> = HashSet::new();
        let changes = ComponentChanges::new(&old, &new);

        assert_eq!(changes.deleted, old);
        assert!(changes.created.is_empty());
        assert!(changes.kept.is_empty());
    }

    #[test]
    fn test_deleted_2() {
        let old = HashSet::from(["a", "b"]).into_iter().map(C::from).collect();
        let new: HashSet<ComponentId> = HashSet::new();
        let changes = ComponentChanges::new(&old, &new);

        assert_eq!(changes.deleted, old);
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
            changes.deleted,
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
