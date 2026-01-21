use async_tempfile::TempDir;
use color_eyre::Result;
use common::fixture::JobAgentFixture;
use orb_jobs_agent::shell::Shell;
use orb_relay_messages::jobs::v1::JobExecutionStatus;
use orb_relay_messages::tonic::async_trait;
use std::process::Stdio;
use tokio::{fs, process::Child};

mod common;

/// A mock shell that returns success for systemctl commands.
/// All other commands are passed through to the host shell.
#[derive(Debug, Clone)]
struct MockShell;

#[async_trait]
impl Shell for MockShell {
    async fn exec(&self, cmd: &[&str]) -> Result<Child> {
        // For systemctl commands, just return success
        if cmd.first() == Some(&"systemctl") {
            Ok(tokio::process::Command::new("true")
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?)
        } else {
            // For other commands, use the host shell
            orb_jobs_agent::shell::Host.exec(cmd).await
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn it_resets_gimbal_on_pearl() {
    // This test verifies the full reset_gimbal flow:
    // 1. Reads and backs up calibration file
    // 2. Updates the calibration with new offsets
    // 3. Restarts worldcoin-core.service
    // 4. Completes successfully

    // Arrange - create temp files for test
    let temp_dir = TempDir::new().await.unwrap();
    let calibration_path = temp_dir.to_path_buf().join("calibration.json");
    let os_release_path = temp_dir.to_path_buf().join("os-release");

    // Create mock calibration file with nested format
    let calibration_content = r#"{
  "mirror": {
    "phi_offset_degrees": 1.0,
    "theta_offset_degrees": 2.0,
    "version": "v2"
  },
  "sensor_id": "test-sensor",
  "extra_field": "should be preserved"
}"#;
    fs::write(&calibration_path, calibration_content)
        .await
        .unwrap();

    // Create mock OS release file (Pearl platform)
    let os_release_content = r#"PRETTY_NAME="Test Orb OS"
ORB_OS_RELEASE_TYPE=dev
ORB_OS_PLATFORM_TYPE=pearl
ORB_OS_EXPECTED_MAIN_MCU_VERSION=v3.0.15
ORB_OS_EXPECTED_SEC_MCU_VERSION=v3.0.15"#;
    fs::write(&os_release_path, os_release_content)
        .await
        .unwrap();

    // Create test fixture with custom paths
    let mut fx = JobAgentFixture::new().await;
    fx.settings.calibration_file_path = calibration_path.clone();
    fx.settings.os_release_path = os_release_path;

    fx.program().shell(MockShell).spawn().await;

    // Act - execute command and wait for completion
    fx.enqueue_job("reset_gimbal")
        .await
        .wait_for_completion()
        .await;

    // Assert - check final status
    let jobs = fx.execution_updates.read().await;
    let result = jobs.last().unwrap();

    assert_eq!(
        result.status,
        JobExecutionStatus::Succeeded as i32,
        "Should succeed. stderr: {}",
        result.std_err
    );

    // Verify backup was created
    let mut backup_files = vec![];
    let mut entries = fs::read_dir(temp_dir.to_path_buf()).await.unwrap();
    while let Ok(Some(entry)) = entries.next_entry().await {
        backup_files.push(entry);
    }
    let has_backup = backup_files.iter().any(|f| {
        let name = f.file_name().to_string_lossy().to_string();
        name.starts_with("calibration.json") && name.ends_with(".bak")
    });
    assert!(has_backup, "Backup file should be created");

    // Verify calibration file was updated
    let updated_content = fs::read_to_string(&calibration_path).await.unwrap();
    assert!(
        updated_content.contains("\"mirror\""),
        "Mirror structure should be preserved"
    );
    assert!(updated_content.contains("\"phi_offset_degrees\": 0.46"));
    assert!(updated_content.contains("\"theta_offset_degrees\": 0.12"));
    assert!(
        updated_content.contains("\"version\": \"v2\""),
        "Other mirror fields should be preserved"
    );
    assert!(
        updated_content.contains("extra_field"),
        "Other top-level fields should be preserved"
    );

    // Verify response contains expected data
    assert!(
        result.std_out.contains("backup"),
        "Response should contain backup filename"
    );
    assert!(
        result.std_out.contains("calibration"),
        "Response should contain calibration data"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn it_fails_on_non_pearl_devices() {
    // Arrange
    let temp_dir = TempDir::new().await.unwrap();
    let calibration_path = temp_dir.to_path_buf().join("calibration.json");
    let os_release_path = temp_dir.to_path_buf().join("os-release");

    let calibration_content =
        r#"{"mirror": {"phi_offset_degrees": 1.0, "theta_offset_degrees": 2.0}}"#;
    fs::write(&calibration_path, calibration_content)
        .await
        .unwrap();

    let os_release_content = r#"PRETTY_NAME="Test Orb OS"
ORB_OS_RELEASE_TYPE=dev
ORB_OS_PLATFORM_TYPE=diamond
ORB_OS_EXPECTED_MAIN_MCU_VERSION=v3.0.15
ORB_OS_EXPECTED_SEC_MCU_VERSION=v3.0.15"#;
    fs::write(&os_release_path, os_release_content)
        .await
        .unwrap();

    let mut fx = JobAgentFixture::new().await;
    fx.settings.calibration_file_path = calibration_path;
    fx.settings.os_release_path = os_release_path;

    fx.program().shell(MockShell).spawn().await;

    // Act
    fx.enqueue_job("reset_gimbal")
        .await
        .wait_for_completion()
        .await;

    // Assert
    let jobs = fx.execution_updates.read().await;
    let result = jobs.last().unwrap();

    assert_eq!(
        result.status,
        JobExecutionStatus::FailedUnsupported as i32,
        "Should fail with unsupported status on non-Pearl devices"
    );
    assert!(
        result.std_err.contains("Pearl"),
        "Error should mention Pearl requirement"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn it_fails_when_calibration_file_missing() {
    // Arrange
    let temp_dir = TempDir::new().await.unwrap();
    let calibration_path = temp_dir.to_path_buf().join("calibration.json");
    let os_release_path = temp_dir.to_path_buf().join("os-release");

    let os_release_content = r#"PRETTY_NAME="Test Orb OS"
ORB_OS_RELEASE_TYPE=dev
ORB_OS_PLATFORM_TYPE=pearl
ORB_OS_EXPECTED_MAIN_MCU_VERSION=v3.0.15
ORB_OS_EXPECTED_SEC_MCU_VERSION=v3.0.15"#;
    fs::write(&os_release_path, os_release_content)
        .await
        .unwrap();

    let mut fx = JobAgentFixture::new().await;
    fx.settings.calibration_file_path = calibration_path;
    fx.settings.os_release_path = os_release_path;

    fx.program().shell(MockShell).spawn().await;

    // Act
    fx.enqueue_job("reset_gimbal")
        .await
        .wait_for_completion()
        .await;

    // Assert
    let jobs = fx.execution_updates.read().await;
    let result = jobs.last().unwrap();

    assert_eq!(
        result.status,
        JobExecutionStatus::Failed as i32,
        "Should fail when calibration file is missing"
    );
    assert!(
        result.std_err.contains("calibration"),
        "Error should mention calibration file. Got: {}",
        result.std_err
    );
}
