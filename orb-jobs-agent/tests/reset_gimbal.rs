use async_tempfile::TempDir;
use common::{fake_orb::FakeOrb, fixture::JobAgentFixture};
use orb_relay_messages::jobs::v1::JobExecutionStatus;
use std::time::Duration;
use tokio::{fs, time};

mod common;

// No docker in macos on github
#[cfg_attr(target_os = "macos", test_with::no_env(GITHUB_ACTIONS))]
#[tokio::test]
async fn it_resets_gimbal_on_pearl() {
    // This test verifies the full reset_gimbal flow:
    // 1. Reads and backs up calibration file
    // 2. Updates the calibration with new offsets
    // 3. Schedules a reboot
    // 4. After simulated reboot, completes successfully

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

    let program_handle = fx.spawn_program(FakeOrb::new().await);
    let reboot_lockfile = fx.settings.store_path.join("reboot.lock");

    // 1. Execute command, should create pending reboot lockfile
    let ticket = fx.enqueue_job("reset_gimbal").await;
    time::sleep(Duration::from_secs(2)).await;

    // Check result
    let jobs = fx.execution_updates.read().await;
    let first_update = jobs.first().unwrap();
    let status = JobExecutionStatus::try_from(first_update.status).unwrap();

    assert_eq!(
        status,
        JobExecutionStatus::InProgress,
        "Should be in progress. stderr: {}",
        first_update.std_err
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

    // Verify lockfile was created
    let pending_execution_id = fs::read_to_string(&reboot_lockfile).await.unwrap();
    assert_eq!(ticket.exec_id, pending_execution_id);

    // 2. Simulate Orb Reboot
    program_handle.stop().await;
    fx.spawn_program(FakeOrb::new().await);

    // 3. Receive command from backend, finish execution
    fx.enqueue_job_with_id("reset_gimbal", ticket.exec_id)
        .await
        .wait_for_completion()
        .await;

    // Assert final state
    let jobs = fx.execution_updates.read().await;
    let last_progress = &jobs[jobs.len() - 2];
    let success = &jobs[jobs.len() - 1];

    assert!(!fs::try_exists(&reboot_lockfile).await.unwrap());
    assert_eq!(success.status, JobExecutionStatus::Succeeded as i32);
    assert_eq!(last_progress.status, JobExecutionStatus::InProgress as i32);
    assert_eq!(last_progress.std_out, "rebooted");
}

// No docker in macos on github  
#[cfg_attr(target_os = "macos", test_with::no_env(GITHUB_ACTIONS))]
#[tokio::test]
async fn it_validates_error_handling() {
    // Note: This test validates error handling when files don't exist

    // Arrange
    let fx = JobAgentFixture::new().await;
    let program_handle = fx.spawn_program(FakeOrb::new().await);

    // Act - execute reset_gimbal command (will fail due to missing files)
    fx.enqueue_job("reset_gimbal")
        .await
        .wait_for_completion()
        .await;

    // Assert - should fail gracefully
    let jobs = fx.execution_updates.read().await;
    let result = jobs.first().unwrap();

    let status = JobExecutionStatus::try_from(result.status).unwrap();

    // Should fail because test environment doesn't have proper Orb OS setup
    assert!(
        status == JobExecutionStatus::Failed
            || status == JobExecutionStatus::FailedUnsupported,
        "Expected failure status in test environment, got: {status:?}"
    );

    // Verify error message is reasonable
    assert!(
        result.std_err.contains("Orb OS release")
            || result.std_err.contains("Pearl")
            || result.std_err.contains("calibration"),
        "Expected error about missing Orb OS setup, got: {}",
        result.std_err
    );

    program_handle.stop().await;
}
