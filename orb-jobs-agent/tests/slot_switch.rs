use color_eyre::Result;
use common::fixture::JobAgentFixture;
use orb_jobs_agent::shell::Shell;
use orb_relay_messages::jobs::v1::JobExecutionStatus;
use std::{sync::Arc, time::Duration};
use tokio::{fs, process::Child, sync::Mutex, time::sleep};

mod common;

#[derive(Debug, Clone)]
struct MockSlotCtrl {
    current_slot: Arc<Mutex<String>>,
}

impl MockSlotCtrl {
    fn new(initial_slot: &str) -> Self {
        Self {
            current_slot: Arc::new(Mutex::new(initial_slot.to_string())),
        }
    }
}

#[async_trait::async_trait]
impl Shell for MockSlotCtrl {
    async fn exec(&self, cmd: &[&str]) -> Result<Child> {
        if cmd.first() == Some(&"orb-slot-ctrl") && cmd.get(1) == Some(&"-c") {
            let slot = self.current_slot.lock().await.clone();
            Ok(tokio::process::Command::new("echo")
                .arg("-n")
                .arg(slot)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()?)
        } else if cmd.len() == 3
            && cmd[0] == "orb-slot-ctrl"
            && cmd[1] == "-s"
        {
            *self.current_slot.lock().await = cmd[2].to_string();
            Ok(tokio::process::Command::new("true")
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()?)
        } else {
            Ok(tokio::process::Command::new("true")
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()?)
        }
    }
}

#[cfg_attr(target_os = "macos", test_with::no_env(GITHUB_ACTIONS))]
#[tokio::test]
async fn switches_from_a_to_b() {
    // Arrange
    let fx = JobAgentFixture::new().await;
    let shell = MockSlotCtrl::new("a");
    let program_handle = fx.program().shell(shell.clone()).spawn().await;
    let reboot_lockfile = fx.settings.store_path.join("reboot.lock");

    // Act
    let ticket = fx.enqueue_job(r#"slot_switch {"slot":"b"}"#).await;
    sleep(Duration::from_secs(1)).await;

    // Assert
    let jobs = fx.execution_updates.read().await;
    let progress = jobs.first().unwrap();
    let pending_execution_id = fs::read_to_string(&reboot_lockfile).await.unwrap();
    assert_eq!(ticket.exec_id, pending_execution_id);
    assert!(progress.std_out.contains("Switched from slot a to slot b"));
    assert_eq!(progress.status, JobExecutionStatus::InProgress as i32);

    // Arrange
    program_handle.stop().await;
    let new_shell = MockSlotCtrl::new("b");
    fx.program().shell(new_shell).spawn().await;

    // Act
    fx.enqueue_job_with_id(r#"slot_switch {"slot":"b"}"#, ticket.exec_id)
        .await
        .wait_for_completion()
        .await;

    // Assert
    let jobs = fx.execution_updates.read().await;
    let success = jobs.last().unwrap();
    assert!(!fs::try_exists(reboot_lockfile).await.unwrap());
    assert_eq!(success.status, JobExecutionStatus::Succeeded as i32);
}

#[cfg_attr(target_os = "macos", test_with::no_env(GITHUB_ACTIONS))]
#[tokio::test]
async fn switches_from_b_to_a() {
    // Arrange
    let fx = JobAgentFixture::new().await;
    let shell = MockSlotCtrl::new("b");
    let program_handle = fx.program().shell(shell.clone()).spawn().await;
    let reboot_lockfile = fx.settings.store_path.join("reboot.lock");

    // Act
    let ticket = fx.enqueue_job(r#"slot_switch {"slot":"a"}"#).await;
    sleep(Duration::from_secs(1)).await;

    // Assert
    let jobs = fx.execution_updates.read().await;
    let progress = jobs.first().unwrap();
    assert!(progress.std_out.contains("Switched from slot b to slot a"));
    assert_eq!(progress.status, JobExecutionStatus::InProgress as i32);

    // Arrange
    program_handle.stop().await;
    let new_shell = MockSlotCtrl::new("a");
    fx.program().shell(new_shell).spawn().await;

    // Act
    fx.enqueue_job_with_id(r#"slot_switch {"slot":"a"}"#, ticket.exec_id)
        .await
        .wait_for_completion()
        .await;

    // Assert
    let jobs = fx.execution_updates.read().await;
    let success = jobs.last().unwrap();
    assert!(!fs::try_exists(reboot_lockfile).await.unwrap());
    assert_eq!(success.status, JobExecutionStatus::Succeeded as i32);
}

#[cfg_attr(target_os = "macos", test_with::no_env(GITHUB_ACTIONS))]
#[tokio::test]
async fn switches_to_other_slot_from_a() {
    // Arrange
    let fx = JobAgentFixture::new().await;
    let shell = MockSlotCtrl::new("a");
    let program_handle = fx.program().shell(shell.clone()).spawn().await;

    // Act
    let ticket = fx.enqueue_job(r#"slot_switch {"slot":"other"}"#).await;
    sleep(Duration::from_secs(1)).await;

    // Assert
    let jobs = fx.execution_updates.read().await;
    let progress = jobs.first().unwrap();
    assert!(progress.std_out.contains("Switched from slot a to slot b"));

    // Arrange
    program_handle.stop().await;
    let new_shell = MockSlotCtrl::new("b");
    fx.program().shell(new_shell).spawn().await;

    // Act
    fx.enqueue_job_with_id(r#"slot_switch {"slot":"other"}"#, ticket.exec_id)
        .await
        .wait_for_completion()
        .await;

    // Assert
    let jobs = fx.execution_updates.read().await;
    let success = jobs.last().unwrap();
    assert_eq!(success.status, JobExecutionStatus::Succeeded as i32);
}

#[cfg_attr(target_os = "macos", test_with::no_env(GITHUB_ACTIONS))]
#[tokio::test]
async fn switches_to_other_slot_from_b() {
    // Arrange
    let fx = JobAgentFixture::new().await;
    let shell = MockSlotCtrl::new("b");
    let program_handle = fx.program().shell(shell.clone()).spawn().await;

    // Act
    let ticket = fx.enqueue_job(r#"slot_switch {"slot":"other"}"#).await;
    sleep(Duration::from_secs(1)).await;

    // Assert
    let jobs = fx.execution_updates.read().await;
    let progress = jobs.first().unwrap();
    assert!(progress.std_out.contains("Switched from slot b to slot a"));

    // Arrange
    program_handle.stop().await;
    let new_shell = MockSlotCtrl::new("a");
    fx.program().shell(new_shell).spawn().await;

    // Act
    fx.enqueue_job_with_id(r#"slot_switch {"slot":"other"}"#, ticket.exec_id)
        .await
        .wait_for_completion()
        .await;

    // Assert
    let jobs = fx.execution_updates.read().await;
    let success = jobs.last().unwrap();
    assert_eq!(success.status, JobExecutionStatus::Succeeded as i32);
}

#[tokio::test]
async fn no_op_when_already_on_target_slot_a() {
    // Arrange
    let fx = JobAgentFixture::new().await;
    let shell = MockSlotCtrl::new("a");
    fx.program().shell(shell).spawn().await;

    // Act
    fx.enqueue_job(r#"slot_switch {"slot":"a"}"#)
        .await
        .wait_for_completion()
        .await;

    // Assert
    let jobs = fx.execution_updates.read().await;
    let result = jobs.first().unwrap();
    assert_eq!(result.status, JobExecutionStatus::Failed as i32);
    assert!(result.std_err.contains("Already on slot a"));
    assert!(result.std_err.contains("nothing to do"));
}

#[tokio::test]
async fn no_op_when_already_on_target_slot_b() {
    // Arrange
    let fx = JobAgentFixture::new().await;
    let shell = MockSlotCtrl::new("b");
    fx.program().shell(shell).spawn().await;

    // Act
    fx.enqueue_job(r#"slot_switch {"slot":"b"}"#)
        .await
        .wait_for_completion()
        .await;

    // Assert
    let jobs = fx.execution_updates.read().await;
    let result = jobs.first().unwrap();
    assert_eq!(result.status, JobExecutionStatus::Failed as i32);
    assert!(result.std_err.contains("Already on slot b"));
    assert!(result.std_err.contains("nothing to do"));
}

#[tokio::test]
async fn fails_on_invalid_slot_argument() {
    // Arrange
    let fx = JobAgentFixture::new().await;
    let shell = MockSlotCtrl::new("a");
    fx.program().shell(shell).spawn().await;

    // Act
    fx.enqueue_job(r#"slot_switch {"slot":"c"}"#)
        .await
        .wait_for_completion()
        .await;

    // Assert
    let jobs = fx.execution_updates.read().await;
    let result = jobs.first().unwrap();
    let status = JobExecutionStatus::try_from(result.status).unwrap();
    assert_eq!(status, JobExecutionStatus::Failed);
}

#[tokio::test]
async fn fails_on_missing_slot_argument() {
    // Arrange
    let fx = JobAgentFixture::new().await;
    let shell = MockSlotCtrl::new("a");
    fx.program().shell(shell).spawn().await;

    // Act
    fx.enqueue_job(r#"slot_switch {}"#)
        .await
        .wait_for_completion()
        .await;

    // Assert
    let jobs = fx.execution_updates.read().await;
    let result = jobs.first().unwrap();
    let status = JobExecutionStatus::try_from(result.status).unwrap();
    assert_eq!(status, JobExecutionStatus::Failed);
}
