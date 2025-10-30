use crate::job_system::ctx::{Ctx, JobExecutionUpdateExt};
use color_eyre::{
    eyre::{bail, Context, ContextCompat},
    Result,
};
use orb_relay_messages::jobs::v1::JobExecutionUpdate;
use serde_json::{json, Value};
use std::path::Path;
use tokio::fs;
use tracing::info;

/// command format: `update-versions <new_version>`
#[tracing::instrument]
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
            create_minimal_versions(new_version)
        }
    };

    write_versions_file(versions_file_path, &updated_versions).await?;

    let output = serde_json::to_string_pretty(&updated_versions)?;

    Ok(ctx
        .success()
        .stdout(format!("Updated versions.json for slot_{current_slot}\n{output}")))
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

    let slot = String::from_utf8_lossy(&output.stdout)
        .trim()
        .to_string();

    if slot != "a" && slot != "b" {
        bail!("unexpected slot value from orb-slot-ctrl: '{}'", slot);
    }

    Ok(slot)
}

async fn read_and_validate_versions(path: impl AsRef<Path>) -> Result<Value> {
    let contents = fs::read_to_string(path)
        .await
        .context("failed to read versions.json")?;

    let data: Value =
        serde_json::from_str(&contents).context("failed to parse versions.json as JSON")?;

    validate_versions_structure(&data)?;

    Ok(data)
}

fn validate_versions_structure(data: &Value) -> Result<()> {
    let obj = data
        .as_object()
        .context("versions.json root is not an object")?;

    let releases = obj
        .get("releases")
        .and_then(|v| v.as_object())
        .context("missing or invalid 'releases' object")?;

    if releases.is_empty() {
        bail!("'releases' object is empty");
    }

    releases
        .get("slot_a")
        .context("missing 'releases.slot_a'")?;

    releases
        .get("slot_b")
        .context("missing 'releases.slot_b'")?;

    let slot_a = obj
        .get("slot_a")
        .and_then(|v| v.as_object())
        .context("missing or invalid 'slot_a' object")?;

    slot_a
        .get("jetson")
        .and_then(|v| v.as_object())
        .context("missing or invalid 'slot_a.jetson' object")?;

    slot_a
        .get("mcu")
        .and_then(|v| v.as_object())
        .context("missing or invalid 'slot_a.mcu' object")?;

    let slot_b = obj
        .get("slot_b")
        .and_then(|v| v.as_object())
        .context("missing or invalid 'slot_b' object")?;

    slot_b
        .get("jetson")
        .and_then(|v| v.as_object())
        .context("missing or invalid 'slot_b.jetson' object")?;

    slot_b
        .get("mcu")
        .and_then(|v| v.as_object())
        .context("missing or invalid 'slot_b.mcu' object")?;

    let singles = obj
        .get("singles")
        .and_then(|v| v.as_object())
        .context("missing or invalid 'singles' object")?;

    singles
        .get("jetson")
        .and_then(|v| v.as_object())
        .context("missing or invalid 'singles.jetson' object")?;

    singles
        .get("mcu")
        .and_then(|v| v.as_object())
        .context("missing or invalid 'singles.mcu' object")?;

    Ok(())
}

fn update_slot_version(data: &mut Value, slot: &str, new_version: &str) -> Result<()> {
    let slot_key = format!("slot_{slot}");
    let new_release = format!("to-{new_version}");

    data.as_object_mut()
        .and_then(|obj| obj.get_mut("releases"))
        .and_then(|releases| releases.as_object_mut())
        .and_then(|releases| releases.get_mut(&slot_key))
        .context("failed to access releases object for update")?;

    data["releases"][&slot_key] = json!(new_release);

    Ok(())
}

fn create_minimal_versions(new_version: &str) -> Value {
    let new_release = format!("to-{new_version}");

    json!({
        "releases": {
            "slot_a": new_release,
            "slot_b": new_release
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
    })
}

async fn write_versions_file(path: impl AsRef<Path>, data: &Value) -> Result<()> {
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
    fn test_validate_valid_minimal_structure() {
        let data = json!({
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
        });

        assert!(validate_versions_structure(&data).is_ok());
    }

    #[test]
    fn test_validate_with_extra_fields() {
        let data = json!({
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
        });

        assert!(validate_versions_structure(&data).is_ok());
    }

    #[test]
    fn test_validate_missing_releases() {
        let data = json!({
            "slot_a": {},
            "slot_b": {},
            "singles": {}
        });

        assert!(validate_versions_structure(&data).is_err());
    }

    #[test]
    fn test_validate_missing_slot() {
        let data = json!({
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
        });

        assert!(validate_versions_structure(&data).is_err());
    }

    #[test]
    fn test_validate_empty_releases() {
        let data = json!({
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
        });

        assert!(validate_versions_structure(&data).is_err());
    }

    #[test]
    fn test_validate_missing_slot_a_jetson() {
        let data = json!({
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
        });

        assert!(validate_versions_structure(&data).is_err());
    }

    #[test]
    fn test_validate_missing_slot_b_mcu() {
        let data = json!({
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
        });

        assert!(validate_versions_structure(&data).is_err());
    }

    #[test]
    fn test_validate_missing_singles_fields() {
        let data = json!({
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
        });

        assert!(validate_versions_structure(&data).is_err());
    }

    #[test]
    fn test_validate_jetson_not_object() {
        let data = json!({
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
        });

        assert!(validate_versions_structure(&data).is_err());
    }

    #[test]
    fn test_update_slot_version_slot_a() {
        let mut data = json!({
            "releases": {
                "slot_a": "old-release",
                "slot_b": "other-release"
            },
            "slot_a": {},
            "slot_b": {},
            "singles": {}
        });

        update_slot_version(&mut data, "a", "v1.2.3").unwrap();

        assert_eq!(data["releases"]["slot_a"], "to-v1.2.3");
        assert_eq!(data["releases"]["slot_b"], "other-release");
    }

    #[test]
    fn test_update_slot_version_slot_b() {
        let mut data = json!({
            "releases": {
                "slot_a": "release-a",
                "slot_b": "old-release"
            },
            "slot_a": {},
            "slot_b": {},
            "singles": {}
        });

        update_slot_version(&mut data, "b", "v2.0.0").unwrap();

        assert_eq!(data["releases"]["slot_a"], "release-a");
        assert_eq!(data["releases"]["slot_b"], "to-v2.0.0");
    }

    #[test]
    fn test_create_minimal_versions() {
        let data = create_minimal_versions("v1.5.0");

        assert_eq!(data["releases"]["slot_a"], "to-v1.5.0");
        assert_eq!(data["releases"]["slot_b"], "to-v1.5.0");
        assert!(data["slot_a"].is_object());
        assert!(data["slot_b"].is_object());
        assert!(data["singles"].is_object());
    }
}
