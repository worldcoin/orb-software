use async_tempfile::TempDir;
use color_eyre::Result;
use common::fixture::JobAgentFixture;
use orb_jobs_agent::shell::{Host, Shell};
use orb_relay_messages::jobs::v1::JobExecutionStatus;
use orb_relay_messages::tonic::async_trait;
use serde_json::Value;
use std::{
    process::Stdio,
    sync::{Arc, Mutex},
};
use tokio::{fs, process::Child};

mod common;

#[derive(Debug, Clone, Default)]
struct MockShell {
    commands: Arc<Mutex<Vec<Vec<String>>>>,
}

impl MockShell {
    fn saw_restart(&self) -> bool {
        self.commands
            .lock()
            .unwrap()
            .iter()
            .any(|cmd| cmd == &["systemctl", "restart", "worldcoin-core.service"])
    }
}

#[async_trait]
impl Shell for MockShell {
    async fn exec(&self, cmd: &[&str]) -> Result<Child> {
        self.commands
            .lock()
            .unwrap()
            .push(cmd.iter().map(|part| (*part).to_string()).collect());

        if cmd.first() == Some(&"systemctl") {
            return Ok(tokio::process::Command::new("true")
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?);
        }

        Host.exec(cmd).await
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn updates_bias_in_valid_current_file() {
    let temp_dir = TempDir::new().await.unwrap();
    let current_path = temp_dir.to_path_buf().join("rgb_focus_calibration.json");

    fs::write(
        &current_path,
        r#"{
  "bias": 0,
  "calibrated": true,
  "samples": 11
}"#,
    )
    .await
    .unwrap();

    let mut fx = JobAgentFixture::new().await;
    fx.settings.rgb_focus_calibration_file_path = current_path.clone();
    let shell = MockShell::default();
    fx.program().shell(shell.clone()).spawn().await;

    fx.enqueue_job("reset_rgb_focus_calibration 146.01668037487582")
        .await
        .wait_for_completion()
        .await;

    let jobs = fx.execution_updates.read().await;
    let result = jobs.last().unwrap();
    assert_eq!(result.status, JobExecutionStatus::Succeeded as i32);

    let updated: Value =
        serde_json::from_str(&fs::read_to_string(&current_path).await.unwrap())
            .unwrap();
    assert!((updated["bias"].as_f64().unwrap() - 146.01668037487582).abs() < 1e-12);
    assert_eq!(updated["calibrated"], true);
    assert_eq!(updated["samples"], 11);

    let response: Value = serde_json::from_str(&result.std_out).unwrap();
    assert_eq!(response["recreated"], false);
    assert!(shell.saw_restart());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn recreates_current_file_when_schema_is_invalid() {
    let temp_dir = TempDir::new().await.unwrap();
    let current_path = temp_dir.to_path_buf().join("rgb_focus_calibration.json");

    fs::write(
        &current_path,
        r#"{
  "bias": 0,
  "calibrated": true
}"#,
    )
    .await
    .unwrap();

    let mut fx = JobAgentFixture::new().await;
    fx.settings.rgb_focus_calibration_file_path = current_path.clone();
    let shell = MockShell::default();
    fx.program().shell(shell.clone()).spawn().await;

    fx.enqueue_job("reset_rgb_focus_calibration 12.5")
        .await
        .wait_for_completion()
        .await;

    let jobs = fx.execution_updates.read().await;
    let result = jobs.last().unwrap();
    assert_eq!(result.status, JobExecutionStatus::Succeeded as i32);

    let updated: Value =
        serde_json::from_str(&fs::read_to_string(&current_path).await.unwrap())
            .unwrap();
    assert!((updated["bias"].as_f64().unwrap() - 12.5).abs() < 1e-12);
    assert_eq!(updated["calibrated"], false);
    assert_eq!(updated["samples"], 0);

    let response: Value = serde_json::from_str(&result.std_out).unwrap();
    assert_eq!(response["recreated"], true);
    assert!(shell.saw_restart());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn recreates_current_file_when_missing() {
    let temp_dir = TempDir::new().await.unwrap();
    let current_path = temp_dir.to_path_buf().join("rgb_focus_calibration.json");

    let mut fx = JobAgentFixture::new().await;
    fx.settings.rgb_focus_calibration_file_path = current_path.clone();
    let shell = MockShell::default();
    fx.program().shell(shell.clone()).spawn().await;

    fx.enqueue_job("reset_rgb_focus_calibration 9")
        .await
        .wait_for_completion()
        .await;

    let jobs = fx.execution_updates.read().await;
    let result = jobs.last().unwrap();
    assert_eq!(result.status, JobExecutionStatus::Succeeded as i32);

    let updated: Value =
        serde_json::from_str(&fs::read_to_string(&current_path).await.unwrap())
            .unwrap();
    assert_eq!(updated["bias"], 9.0);
    assert_eq!(updated["calibrated"], false);
    assert_eq!(updated["samples"], 0);

    let response: Value = serde_json::from_str(&result.std_out).unwrap();
    assert_eq!(response["recreated"], true);
    assert!(shell.saw_restart());
}
