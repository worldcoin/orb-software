//! Types involved in describing a [`DiffPlan`].

use std::{collections::HashSet, path::PathBuf};

use orb_update_agent_core::MimeType;
use tracing::{debug, warn};

use super::ota_dir::{OtaDir, OutDir, SourceInfo};

pub const CLAIM_FILE: &str = "claim.json";

#[derive(Debug, Eq, PartialEq, Hash, Clone, derive_more::From)]
pub struct ComponentId(pub String);

impl From<&str> for ComponentId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
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

impl Operation {
    pub fn id(&self) -> &ComponentId {
        match self {
            Operation::Bidiff { id, .. } => id,
            Operation::Copy { id, .. } => id,
        }
    }
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

#[cfg(test)]
mod test_filter_valid_sqfs {
    // TODO(@thebutlah): write tests
}

#[cfg(test)]
mod test_diff_plan {
    use std::{collections::HashMap, path::Path};

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
