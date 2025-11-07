use async_tempfile::TempFile;
use common::fixture::JobAgentFixture;
use orb_jobs_agent::shell::{Host, Shell};
use orb_relay_messages::tonic::async_trait;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tokio::{fs, process::Child};

mod common;

#[derive(Debug, Clone)]
struct MockShell {
    slot: Arc<Mutex<String>>,
}

impl MockShell {
    fn new(slot: &str) -> Self {
        Self {
            slot: Arc::new(Mutex::new(slot.to_string())),
        }
    }
}

#[async_trait]
impl Shell for MockShell {
    async fn exec(&self, cmd: &[&str]) -> color_eyre::Result<Child> {
        if cmd.first() == Some(&"orb-slot-ctrl") && cmd.get(1) == Some(&"-c") {
            let slot = self.slot.lock().unwrap().clone();
            let mock_output = format!("{slot}\n");

            Ok(tokio::process::Command::new("echo")
                .arg("-n")
                .arg(mock_output)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()?)
        } else {
            Host.exec(cmd).await
        }
    }
}

#[tokio::test]
async fn updates_existing_valid_versions_file_slot_a() {
    let temp_file = TempFile::new().await.unwrap();
    let filepath = temp_file.file_path().to_string_lossy().to_string();

    let initial_versions = json!({
        "releases": {
            "slot_a": "old-release-a",
            "slot_b": "old-release-b"
        },
        "slot_a": {
            "jetson": {"version": "1.0"},
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

    fs::write(
        &filepath,
        serde_json::to_string_pretty(&initial_versions).unwrap(),
    )
    .await
    .unwrap();

    let mut fx = JobAgentFixture::new().await;
    fx.settings.versions_file_path = filepath.clone().into();
    fx.program().shell(MockShell::new("a")).spawn().await;

    fx.enqueue_job("update_versions v1.5.0")
        .await
        .wait_for_completion()
        .await;

    let result = fs::read_to_string(&filepath).await.unwrap();
    let updated: Value = serde_json::from_str(&result).unwrap();

    assert_eq!(updated["releases"]["slot_a"], "v1.5.0");
    assert_eq!(updated["releases"]["slot_b"], "old-release-b");
    assert_eq!(updated["slot_a"]["jetson"]["version"], "1.0");
}

#[tokio::test]
async fn updates_existing_valid_versions_file_slot_b() {
    let temp_file = TempFile::new().await.unwrap();
    let filepath = temp_file.file_path().to_string_lossy().to_string();

    let initial_versions = json!({
        "releases": {
            "slot_a": "release-a",
            "slot_b": "old-release-b"
        },
        "slot_a": {
            "jetson": {},
            "mcu": {}
        },
        "slot_b": {
            "jetson": {},
            "mcu": {"firmware": "2.0"}
        },
        "singles": {
            "jetson": {},
            "mcu": {}
        }
    });

    fs::write(
        &filepath,
        serde_json::to_string_pretty(&initial_versions).unwrap(),
    )
    .await
    .unwrap();

    let mut fx = JobAgentFixture::new().await;
    fx.settings.versions_file_path = filepath.clone().into();
    fx.program().shell(MockShell::new("b")).spawn().await;

    fx.enqueue_job("update_versions v2.0.0")
        .await
        .wait_for_completion()
        .await;

    let result = fs::read_to_string(&filepath).await.unwrap();
    let updated: Value = serde_json::from_str(&result).unwrap();

    assert_eq!(updated["releases"]["slot_a"], "release-a");
    assert_eq!(updated["releases"]["slot_b"], "v2.0.0");
    assert_eq!(updated["slot_b"]["mcu"]["firmware"], "2.0");
}

#[tokio::test]
async fn creates_minimal_structure_when_file_missing() {
    let temp_file = TempFile::new().await.unwrap();
    let filepath = temp_file.file_path().to_string_lossy().to_string();

    let _ = fs::remove_file(&filepath).await;

    let mut fx = JobAgentFixture::new().await;
    fx.settings.versions_file_path = filepath.clone().into();
    fx.program().shell(MockShell::new("a")).spawn().await;

    fx.enqueue_job("update_versions v1.0.0")
        .await
        .wait_for_completion()
        .await;

    let result = fs::read_to_string(&filepath).await.unwrap();
    let updated: Value = serde_json::from_str(&result).unwrap();

    assert_eq!(updated["releases"]["slot_a"], "v1.0.0");
    assert_eq!(updated["releases"]["slot_b"], "unknown");
    assert!(updated["slot_a"].is_object());
    assert!(updated["slot_b"].is_object());
    assert!(updated["singles"].is_object());
}

#[tokio::test]
async fn creates_minimal_structure_when_file_invalid_json() {
    let temp_file = TempFile::new().await.unwrap();
    let filepath = temp_file.file_path().to_string_lossy().to_string();

    fs::write(&filepath, "not valid json at all!")
        .await
        .unwrap();

    let mut fx = JobAgentFixture::new().await;
    fx.settings.versions_file_path = filepath.clone().into();
    fx.program().shell(MockShell::new("a")).spawn().await;

    fx.enqueue_job("update_versions v3.0.0")
        .await
        .wait_for_completion()
        .await;

    let result = fs::read_to_string(&filepath).await.unwrap();
    let updated: Value = serde_json::from_str(&result).unwrap();

    assert_eq!(updated["releases"]["slot_a"], "v3.0.0");
    assert_eq!(updated["releases"]["slot_b"], "unknown");
}

#[tokio::test]
async fn creates_minimal_structure_when_file_missing_required_fields() {
    let temp_file = TempFile::new().await.unwrap();
    let filepath = temp_file.file_path().to_string_lossy().to_string();

    let broken_versions = json!({
        "releases": {
            "slot_a": "something"
        }
    });

    fs::write(
        &filepath,
        serde_json::to_string_pretty(&broken_versions).unwrap(),
    )
    .await
    .unwrap();

    let mut fx = JobAgentFixture::new().await;
    fx.settings.versions_file_path = filepath.clone().into();
    fx.program().shell(MockShell::new("b")).spawn().await;

    fx.enqueue_job("update_versions v4.0.0")
        .await
        .wait_for_completion()
        .await;

    let result = fs::read_to_string(&filepath).await.unwrap();
    let updated: Value = serde_json::from_str(&result).unwrap();

    assert_eq!(updated["releases"]["slot_a"], "unknown");
    assert_eq!(updated["releases"]["slot_b"], "v4.0.0");
    assert!(updated["singles"].is_object());
}

#[tokio::test]
async fn fails_when_no_version_argument_provided() {
    let temp_file = TempFile::new().await.unwrap();
    let filepath = temp_file.file_path().to_string_lossy().to_string();

    let mut fx = JobAgentFixture::new().await;
    fx.settings.versions_file_path = filepath.into();
    fx.program().shell(MockShell::new("a")).spawn().await;

    fx.enqueue_job("update_versions")
        .await
        .wait_for_completion()
        .await;

    let results = fx.execution_updates.map_iter(|x| x.std_err).await;
    assert!(results[0].contains("no version argument"));
}
