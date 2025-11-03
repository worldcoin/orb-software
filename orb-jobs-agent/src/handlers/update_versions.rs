use crate::job_system::ctx::{Ctx, JobExecutionUpdateExt};
use color_eyre::{
    eyre::{bail, Context, ContextCompat},
    Result,
};
use orb_relay_messages::jobs::v1::JobExecutionUpdate;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::Path};
use tokio::fs;
use tracing::info;

#[derive(Deserialize, Serialize, Debug, PartialEq, Eq)]
struct Versions {
    releases: SlotReleases,
    slot_a: VersionGroup,
    slot_b: VersionGroup,
    singles: VersionGroup,
}

#[derive(Deserialize, Serialize, Debug, PartialEq, Eq)]
struct SlotReleases {
    slot_a: String,
    slot_b: String,
}

#[derive(Deserialize, Serialize, Debug, Default, PartialEq, Eq)]
struct VersionGroup {
    #[serde(skip_serializing_if = "HashMap::is_empty", default)]
    jetson: HashMap<String, String>,
    #[serde(skip_serializing_if = "HashMap::is_empty", default)]
    mcu: HashMap<String, String>,
}

/// command format: `update-versions <new_version>`
#[tracing::instrument(skip(ctx))]
pub async fn handler(ctx: Ctx) -> Result<JobExecutionUpdate> {
    let new_version = ctx
        .args()
        .first()
        .filter(|arg| !arg.trim().is_empty())
        .context("no version argument provided")?;

    info!(
        "Updating versions file with new version: {} for job {}",
        new_version,
        ctx.execution_id()
    );

    let versions_file_path = &ctx.deps().settings.versions_file_path;

    let current_slot = get_current_slot(&ctx).await?;
    info!("Current slot: {}", current_slot);

    let versions_data = read_and_validate_versions(versions_file_path).await;

    let updated_versions = match versions_data {
        Ok(mut data) => {
            info!("Valid versions.json found, updating slot_{}", current_slot);
            update_slot_version(&mut data, &current_slot, new_version)?;
            data
        }
        Err(e) => {
            info!(
                "Invalid or missing versions.json ({}), creating minimal structure",
                e
            );
            create_minimal_versions(new_version, &current_slot)
        }
    };

    write_versions_file(versions_file_path, &updated_versions).await?;

    let output = serde_json::to_string_pretty(&updated_versions)?;

    Ok(ctx.success().stdout(format!(
        "Updated versions.json for slot_{current_slot}\n{output}"
    )))
}

async fn get_current_slot(ctx: &Ctx) -> Result<String> {
    let output = ctx
        .deps()
        .shell
        .exec(&["orb-slot-ctrl", "-c"])
        .await
        .context("failed to spawn orb-slot-ctrl")?
        .wait_with_output()
        .await
        .context("failed to wait for orb-slot-ctrl")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("orb-slot-ctrl failed: {}", stderr);
    }

    let slot = String::from_utf8_lossy(&output.stdout).trim().to_string();

    if slot != "a" && slot != "b" {
        bail!("unexpected slot value from orb-slot-ctrl: '{}'", slot);
    }

    Ok(slot)
}

async fn read_and_validate_versions(path: impl AsRef<Path>) -> Result<Versions> {
    let contents = fs::read_to_string(path)
        .await
        .context("failed to read versions.json")?;

    serde_json::from_str(&contents).context("failed to parse versions.json")
}

fn update_slot_version(
    data: &mut Versions,
    slot: &str,
    new_version: &str,
) -> Result<()> {
    match slot {
        "a" => data.releases.slot_a = new_version.to_string(),
        "b" => data.releases.slot_b = new_version.to_string(),
        _ => bail!("unexpected slot value: '{}'", slot),
    }

    Ok(())
}

fn create_minimal_versions(new_version: &str, current_slot: &str) -> Versions {
    let (slot_a_version, slot_b_version) = match current_slot {
        "a" => (new_version.to_string(), "unknown".to_string()),
        "b" => ("unknown".to_string(), new_version.to_string()),

        // Should not happen if the orb is healthy
        _ => (new_version.to_string(), new_version.to_string()),
    };

    Versions {
        releases: SlotReleases {
            slot_a: slot_a_version,
            slot_b: slot_b_version,
        },
        slot_a: VersionGroup::default(),
        slot_b: VersionGroup::default(),
        singles: VersionGroup::default(),
    }
}

async fn write_versions_file(path: impl AsRef<Path>, data: &Versions) -> Result<()> {
    let json_string = serde_json::to_string_pretty(data)
        .context("failed to serialize versions.json")?;

    fs::write(path, json_string)
        .await
        .context("failed to write versions.json")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_valid_minimal_structure() {
        let json_str = r#"{
            "releases": {
                "slot_a": "release-a",
                "slot_b": "release-b"
            },
            "slot_a": {
                "jetson": {},
                "mcu": {}
            },
            "slot_b": {
                "jetson": {},
                "mcu": {}
            },
            "singles": {
                "jetson": {},
                "mcu": {}
            }
        }"#;

        assert!(serde_json::from_str::<Versions>(json_str).is_ok());
    }

    #[test]
    fn test_deserialize_with_extra_fields() {
        let json_str = r#"{
            "releases": {
                "slot_a": "release-a",
                "slot_b": "release-b"
            },
            "slot_a": {
                "jetson": {"version": "1.0"},
                "mcu": {"firmware": "2.0"}
            },
            "slot_b": {
                "jetson": {},
                "mcu": {}
            },
            "singles": {
                "jetson": {},
                "mcu": {}
            }
        }"#;

        assert!(serde_json::from_str::<Versions>(json_str).is_ok());
    }

    #[test]
    fn test_deserialize_missing_releases() {
        let json_str = r#"{
            "slot_a": {},
            "slot_b": {},
            "singles": {}
        }"#;

        assert!(serde_json::from_str::<Versions>(json_str).is_err());
    }

    #[test]
    fn test_deserialize_missing_slot() {
        let json_str = r#"{
            "releases": {
                "slot_a": "release-a",
                "slot_b": "release-b"
            },
            "slot_a": {
                "jetson": {},
                "mcu": {}
            },
            "singles": {
                "jetson": {},
                "mcu": {}
            }
        }"#;

        assert!(serde_json::from_str::<Versions>(json_str).is_err());
    }

    #[test]
    fn test_deserialize_empty_releases() {
        let json_str = r#"{
            "releases": {},
            "slot_a": {
                "jetson": {},
                "mcu": {}
            },
            "slot_b": {
                "jetson": {},
                "mcu": {}
            },
            "singles": {
                "jetson": {},
                "mcu": {}
            }
        }"#;

        assert!(serde_json::from_str::<Versions>(json_str).is_err());
    }

    #[test]
    fn test_deserialize_missing_slot_a_jetson() {
        let json_str = r#"{
            "releases": {
                "slot_a": "release-a",
                "slot_b": "release-b"
            },
            "slot_a": {
                "mcu": {}
            },
            "slot_b": {
                "jetson": {},
                "mcu": {}
            },
            "singles": {
                "jetson": {},
                "mcu": {}
            }
        }"#;

        let result = serde_json::from_str::<Versions>(json_str);
        assert!(result.is_ok());
        assert!(result.unwrap().slot_a.jetson.is_empty());
    }

    #[test]
    fn test_deserialize_missing_slot_b_mcu() {
        let json_str = r#"{
            "releases": {
                "slot_a": "release-a",
                "slot_b": "release-b"
            },
            "slot_a": {
                "jetson": {},
                "mcu": {}
            },
            "slot_b": {
                "jetson": {}
            },
            "singles": {
                "jetson": {},
                "mcu": {}
            }
        }"#;

        let result = serde_json::from_str::<Versions>(json_str);
        assert!(result.is_ok());
        assert!(result.unwrap().slot_b.mcu.is_empty());
    }

    #[test]
    fn test_deserialize_missing_singles_fields() {
        let json_str = r#"{
            "releases": {
                "slot_a": "release-a",
                "slot_b": "release-b"
            },
            "slot_a": {
                "jetson": {},
                "mcu": {}
            },
            "slot_b": {
                "jetson": {},
                "mcu": {}
            },
            "singles": {}
        }"#;

        let result = serde_json::from_str::<Versions>(json_str);
        assert!(result.is_ok());
        let versions = result.unwrap();
        assert!(versions.singles.jetson.is_empty());
        assert!(versions.singles.mcu.is_empty());
    }

    #[test]
    fn test_deserialize_jetson_not_object() {
        let json_str = r#"{
            "releases": {
                "slot_a": "release-a",
                "slot_b": "release-b"
            },
            "slot_a": {
                "jetson": "not an object",
                "mcu": {}
            },
            "slot_b": {
                "jetson": {},
                "mcu": {}
            },
            "singles": {
                "jetson": {},
                "mcu": {}
            }
        }"#;

        assert!(serde_json::from_str::<Versions>(json_str).is_err());
    }

    #[test]
    fn test_update_slot_version_slot_a() {
        let mut data = Versions {
            releases: SlotReleases {
                slot_a: "old-release".to_string(),
                slot_b: "other-release".to_string(),
            },
            slot_a: VersionGroup::default(),
            slot_b: VersionGroup::default(),
            singles: VersionGroup::default(),
        };

        update_slot_version(&mut data, "a", "v1.2.3").unwrap();

        assert_eq!(data.releases.slot_a, "v1.2.3");
        assert_eq!(data.releases.slot_b, "other-release");
    }

    #[test]
    fn test_update_slot_version_slot_b() {
        let mut data = Versions {
            releases: SlotReleases {
                slot_a: "release-a".to_string(),
                slot_b: "old-release".to_string(),
            },
            slot_a: VersionGroup::default(),
            slot_b: VersionGroup::default(),
            singles: VersionGroup::default(),
        };

        update_slot_version(&mut data, "b", "v2.0.0").unwrap();

        assert_eq!(data.releases.slot_a, "release-a");
        assert_eq!(data.releases.slot_b, "v2.0.0");
    }

    #[test]
    fn test_create_minimal_versions_slot_a() {
        let data = create_minimal_versions("v1.5.0", "a");

        assert_eq!(data.releases.slot_a, "v1.5.0");
        assert_eq!(data.releases.slot_b, "unknown");
        assert!(data.slot_a.jetson.is_empty());
        assert!(data.slot_a.mcu.is_empty());
        assert!(data.slot_b.jetson.is_empty());
        assert!(data.slot_b.mcu.is_empty());
        assert!(data.singles.jetson.is_empty());
        assert!(data.singles.mcu.is_empty());
    }

    #[test]
    fn test_create_minimal_versions_slot_b() {
        let data = create_minimal_versions("v2.0.0", "b");

        assert_eq!(data.releases.slot_a, "unknown");
        assert_eq!(data.releases.slot_b, "v2.0.0");
        assert!(data.slot_a.jetson.is_empty());
        assert!(data.slot_a.mcu.is_empty());
        assert!(data.slot_b.jetson.is_empty());
        assert!(data.slot_b.mcu.is_empty());
        assert!(data.singles.jetson.is_empty());
        assert!(data.singles.mcu.is_empty());
    }
}
