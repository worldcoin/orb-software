use common::{fake_orb::FakeOrb, fixture::JobAgentFixture};
use orb_relay_messages::jobs::v1::JobExecutionStatus;
use std::time::Duration;
use tokio::{fs, time};

mod common;

// No docker in macos on github
#[cfg_attr(target_os = "macos", test_with::no_env(GITHUB_ACTIONS))]
#[tokio::test]
async fn it_reboots() {
    // Arrange
    let fx = JobAgentFixture::new().await;
    let program_handle = fx.spawn_program(FakeOrb::new().await);

    let reboot_lockfile = fx.settings.store_path.join("reboot.lock");

    // 1. Executes command, creates pending reboot lockfile
    let execution_id = fx.enqueue_job("reboot").await;
    time::sleep(Duration::from_millis(200)).await; // give enough time exec cmd

    // Assert
    let jobs = fx.execution_updates.read().await;
    let actual = jobs.first().unwrap();

    let pending_execution_id = fs::read_to_string(&reboot_lockfile).await.unwrap();

    assert_eq!(execution_id, pending_execution_id);
    assert_eq!(actual.std_out, "rebooting");
    assert_eq!(actual.status, JobExecutionStatus::InProgress as i32);

    // 2. Simulate Orb Reboot
    program_handle.stop().await;
    fx.spawn_program(FakeOrb::new().await);

    // 3. Receive command from backend, finish execution -- lockfile should be removed
    fx.enqueue_job_with_id("reboot", execution_id).await;
    time::sleep(Duration::from_millis(200)).await; // give enough time exec cmd

    // Assert
    let jobs = fx.execution_updates.read().await;
    let last_progress = &jobs[jobs.len() - 2];
    let success = &jobs[jobs.len() - 1];

    assert!(!fs::try_exists(reboot_lockfile).await.unwrap());
    assert_eq!(success.status, JobExecutionStatus::Succeeded as i32);
    assert_eq!(last_progress.status, JobExecutionStatus::InProgress as i32);
    assert_eq!(last_progress.std_out, "rebooted");
}
