use std::collections::HashMap;

use orb_update_agent_core::{MimeType, Source, UncheckedClaim};

use crate::{
    diff_plan::{DiffPlan, Operation},
    execute_plan::DiffPlanOutputs,
};

/// Newtype on [`UnckechedClaim`] for claims that have been patched by
/// [`patch_claim()`].
#[derive(Debug)]
pub struct PatchedClaim(pub UncheckedClaim);

/// Patches the new/top OTA claim with the computed plan. Essentially this is
/// responsible for modifying the claim sources to account for any bidiff operations.
///
/// # Preconditions
/// `plan` was generated with `new_claim` coming from the newer `OtaDir`.
pub fn patch_claim(
    plan: &DiffPlan,
    plan_outputs: &DiffPlanOutputs,
    new_claim: &UncheckedClaim,
) -> PatchedClaim {
    let mut output_claim = new_claim.clone();
    patch_claim_helper(plan, plan_outputs, &mut output_claim.sources);

    PatchedClaim(output_claim)
}

/// Helper function that represents the bulk of the work of [`patch_claim`] but which is
/// a little bit more testable by virtue of not needing a fully formed
/// [`UncheckedClaim`].
pub fn patch_claim_helper(
    plan: &DiffPlan,
    plan_outputs: &DiffPlanOutputs,
    sources: &mut HashMap<String, Source>,
) {
    assert_eq!(
        plan.ops.len(),
        sources.len(),
        "precondition failed: sources length didn't match plan length"
    );
    for op in plan.ops.iter() {
        let op_component_id = op.id();
        let source = sources.get(&op_component_id.0).expect(
            "precondition failed: sources names didnt match plan component ids",
        );
        assert_eq!(
            &op_component_id.0, &source.name,
            "sanity: map key should always match source name"
        );
        let summary = &plan_outputs.summaries[op_component_id];
        if matches!(op, Operation::Copy { .. }) {
            let summary_hash = hex::encode(&summary.hash);
            assert_eq!(
                summary_hash, source.hash,
                "precondition failed: source hash didn't match hash from plan output"
            );
            assert_eq!(
                summary.size, source.size,
                "precondition failed: source size didn't match plan output size"
            );
        }
    }

    let bidiffs = plan
        .ops
        .iter()
        .filter(|op| matches!(op, Operation::Bidiff { .. }));
    for op in bidiffs {
        let Operation::Bidiff { id, .. } = op else {
            unreachable!("we already filtered to only bidiffs");
        };
        let src = sources
            .get_mut(&id.0)
            .expect("infallible: bidiff components always exist in new_claim");

        let summary = &plan_outputs.summaries[op.id()];
        let summary_hash = hex::encode(&summary.hash);
        src.hash = summary_hash;
        src.size = summary.size;
        src.mime_type = MimeType::ZstdBidiff;
    }
}

#[cfg(test)]
mod test_patch_claim {
    use std::{
        collections::HashSet,
        path::{Path, PathBuf},
    };

    use crate::execute_plan::FileSummary;

    use super::*;
    use orb_update_agent_core::LocalOrRemote;
    use test_log::test;

    fn copy_src(name: impl AsRef<str>, path: impl AsRef<Path>) -> Source {
        Source {
            name: name.as_ref().into(),
            url: LocalOrRemote::Local(path.as_ref().to_path_buf()),
            mime_type: MimeType::ZstdBidiff,
            size: 0,
            hash: "".into(),
        }
    }

    fn bidiff_src(name: impl AsRef<str>, path: impl AsRef<Path>) -> Source {
        Source {
            name: name.as_ref().into(),
            url: LocalOrRemote::Local(path.as_ref().to_path_buf()),
            mime_type: MimeType::ZstdBidiff,
            size: 0,
            hash: "".into(),
        }
    }

    fn check_bidiff_patch(
        original_sources: &HashMap<String, Source>,
        patched: &Source,
    ) {
        // Validate mime
        assert_eq!(
            patched.mime_type,
            MimeType::ZstdBidiff,
            "bidiff sources all have the application/zstd-bidff mime type"
        );

        // Validate name
        let original_source = original_sources
            .get(&patched.name)
            .expect("names should be unchanged from the original");

        // Validate URL
        assert_eq!(
            patched.url, original_source.url,
            "url should match the original"
        );

        // Validate Hash
        {
            // TODO(ORBS-382): Handle hashes
        }

        // Validate Size
        {
            // TODO(ORBS_382): Handle sizes
        }
    }

    #[test]
    fn test_no_ops_empty_sources() {
        // Arrange
        let empty_plan = DiffPlan {
            ops: HashSet::new(),
        };
        let empty_outputs = DiffPlanOutputs {
            summaries: HashMap::new(),
        };
        let mut empty_sources = HashMap::new();
        // Act
        patch_claim_helper(&empty_plan, &empty_outputs, &mut empty_sources);
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
        let empty_outputs = DiffPlanOutputs {
            summaries: HashMap::new(),
        };
        let populated_sources = HashMap::from([("a".into(), copy_src("a", "a.cmp"))]);
        let mut patched_sources = populated_sources.clone();
        // Act (should panic)
        patch_claim_helper(&empty_plan, &empty_outputs, &mut patched_sources);
    }

    #[test]
    #[should_panic]
    fn test_source_that_doesnt_appear_in_plan_panics_due_to_precondition() {
        // Arrange
        let plan = DiffPlan {
            ops: HashSet::from([Operation::Copy {
                id: "a".into(),
                from_path: "/from/a.cmp".into(),
                to_path: "/to/a.cmp".into(),
            }]),
        };
        let plan_outputs = DiffPlanOutputs {
            summaries: HashMap::from([(
                "a".into(),
                FileSummary {
                    hash: vec![],
                    size: 0,
                },
            )]),
        };
        let populated_sources = HashMap::from([
            ("a".into(), copy_src("a", "a.cmp")), // exists in plan
            ("b".into(), copy_src("b", "b.cmp")), // doesnt exist in plan
        ]);
        let mut patched_sources = populated_sources.clone();
        // Act (should panic)
        patch_claim_helper(&plan, &plan_outputs, &mut patched_sources);
    }

    #[test]
    fn test_only_copy_ops() {
        // Arrange
        let a_to = PathBuf::from("/to/a.cmp");
        let b_to = PathBuf::from("/to/b.cmp");
        let only_copy_ops = DiffPlan {
            ops: HashSet::from([
                Operation::Copy {
                    id: "a".into(),
                    from_path: "/from/a.cmp".into(),
                    to_path: a_to.clone(),
                },
                Operation::Copy {
                    id: "b".into(),
                    from_path: "/from/b.cmp".into(),
                    to_path: b_to.clone(),
                },
            ]),
        };
        let plan_outputs = DiffPlanOutputs {
            summaries: HashMap::from([
                (
                    "a".into(),
                    FileSummary {
                        hash: vec![],
                        size: 0,
                    },
                ),
                (
                    "b".into(),
                    FileSummary {
                        hash: vec![],
                        size: 0,
                    },
                ),
            ]),
        };
        let populated_sources = HashMap::from([
            ("a".into(), copy_src("a", "a.cmp")),
            ("b".into(), copy_src("b", "b.cmp")),
        ]);
        let mut patched_sources = populated_sources.clone();

        // Act
        patch_claim_helper(&only_copy_ops, &plan_outputs, &mut patched_sources);

        // Assert
        assert_eq!(
            patched_sources, populated_sources,
            "only had copy ops, so nothing should have changed"
        );
    }

    #[test]
    fn test_only_bidiff_ops() {
        // Arrange
        let a_new = PathBuf::from("/new/a.cmp");
        let a_out = PathBuf::from("/out/a.cmp");
        let b_new = PathBuf::from("/new/b.cmp");
        let b_out = PathBuf::from("/out/b.cmp");
        let only_bidiff_ops = DiffPlan {
            ops: HashSet::from([
                Operation::Bidiff {
                    id: "a".into(),
                    old_path: "/old/a.cmp".into(),
                    new_path: a_new.clone(),
                    out_path: a_out.clone(),
                },
                Operation::Bidiff {
                    id: "b".into(),
                    old_path: "/old/b.cmp".into(),
                    new_path: b_new.clone(),
                    out_path: b_out.clone(),
                },
            ]),
        };
        let plan_outputs = DiffPlanOutputs {
            summaries: HashMap::from([
                (
                    "a".into(),
                    FileSummary {
                        hash: vec![],
                        size: 0,
                    },
                ),
                (
                    "b".into(),
                    FileSummary {
                        hash: vec![],
                        size: 0,
                    },
                ),
            ]),
        };
        let original_sources = HashMap::from([
            ("a".into(), bidiff_src("a", "a.cmp")),
            ("b".into(), bidiff_src("b", "b.cmp")),
        ]);
        let mut patched_sources = original_sources.clone();

        // Act
        patch_claim_helper(&only_bidiff_ops, &plan_outputs, &mut patched_sources);

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
            check_bidiff_patch(&original_sources, &patched_source);
        }
    }

    #[test]
    fn test_mixed_ops() {
        // Arrange
        let only_copy_ops = DiffPlan {
            ops: HashSet::from([
                Operation::Copy {
                    id: "copy".into(),
                    from_path: "/from/copy.cmp".into(),
                    to_path: "/to/copy.cmp".into(),
                },
                Operation::Bidiff {
                    id: "bidiff".into(),
                    old_path: "/old/bidiff.cmp".into(),
                    new_path: "/new/bidiff.cmp".into(),
                    out_path: "/out/bidiff.cmp".into(),
                },
            ]),
        };
        let plan_outputs = DiffPlanOutputs {
            summaries: HashMap::from([
                (
                    "copy".into(),
                    FileSummary {
                        hash: vec![],
                        size: 0,
                    },
                ),
                (
                    "bidiff".into(),
                    FileSummary {
                        hash: vec![],
                        size: 0,
                    },
                ),
            ]),
        };
        let original_sources = HashMap::from([
            ("copy".into(), copy_src("copy", "copy.cmp")),
            ("bidiff".into(), bidiff_src("bidiff", "bidiff.cmp")),
        ]);
        let mut patched_sources = original_sources.clone();

        // Act
        patch_claim_helper(&only_copy_ops, &plan_outputs, &mut patched_sources);

        // Assert
        assert!(
            patched_sources.iter().all(|(key, val)| key == &val.name),
            "sanity: all keys much match source names in patch"
        );
        assert_eq!(
            patched_sources.len(),
            original_sources.len(),
            "should have same # of sources as original"
        );
        assert_eq!(
            original_sources["copy"], patched_sources["copy"],
            "copy op should not change source"
        );
        check_bidiff_patch(&original_sources, &patched_sources["bidiff"])
    }
}
