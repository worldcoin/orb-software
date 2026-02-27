use color_eyre::Result;
use common::fixture::JobAgentFixture;
use orb_jobs_agent::shell::Shell;
use orb_relay_messages::{jobs::v1::JobExecutionStatus, tonic::async_trait};
use std::sync::{Arc, Mutex};
use tokio::{fs, process::Child};

mod common;

#[derive(Debug, Clone)]
struct CommandTracker {
    current_slot: String,
    commands: Arc<Mutex<Vec<String>>>,
}

impl CommandTracker {
    fn new(current_slot: &str) -> Self {
        Self {
            current_slot: current_slot.to_string(),
            commands: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn commands(&self) -> Vec<String> {
        self.commands.lock().unwrap().clone()
    }
}

#[async_trait]
impl Shell for CommandTracker {
    async fn exec(&self, cmd: &[&str]) -> Result<Child> {
        let cmd_str = cmd.join(" ");
        self.commands.lock().unwrap().push(cmd_str.clone());

        if cmd.first() == Some(&"orb-slot-ctrl") && cmd.get(1) == Some(&"-c") {
            Ok(tokio::process::Command::new("echo")
                .arg("-n")
                .arg(&self.current_slot)
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

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn switches_from_a_to_b() {
    let fx = JobAgentFixture::new().await;
    let shell = CommandTracker::new("a");
    fx.program().shell(shell.clone()).spawn().await;
    let reboot_lockfile = fx.settings.store_path.join("reboot.lock");

    let ticket = fx.enqueue_job(r#"slot_switch {"slot":"b"}"#).await;
    fx.wait_for_updates(1).await;

    // Assert: Correct slot commands were executed
    let commands = shell.commands();
    assert!(
        commands.contains(&"orb-slot-ctrl -c".to_string()),
        "Should check current slot"
    );
    assert!(
        commands.contains(&"orb-slot-ctrl -s b".to_string()),
        "Should switch to slot b"
    );

    // Assert: Reboot flow was initiated (InProgress status indicates reboot is pending)
    let jobs = fx.execution_updates.read().await;
    let progress = jobs.first().expect("Should have at least one update");
    assert_eq!(
        progress.status,
        JobExecutionStatus::InProgress as i32,
        "Should be in progress (waiting for reboot)"
    );
    assert!(
        progress.std_out.contains("Switched from slot a to slot b"),
        "Should report slot switch"
    );

    // Assert: Lockfile was created with correct exec_id (proves reboot flow was initiated)
    let lockfile_content = fs::read_to_string(&reboot_lockfile)
        .await
        .expect("Lockfile should exist");
    assert_eq!(lockfile_content, ticket.exec_id);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn switches_from_b_to_a() {
    let fx = JobAgentFixture::new().await;
    let shell = CommandTracker::new("b");
    fx.program().shell(shell.clone()).spawn().await;

    fx.enqueue_job(r#"slot_switch {"slot":"a"}"#)
        .await;
    fx.wait_for_updates(1).await;

    let commands = shell.commands();
    assert!(
        commands.contains(&"orb-slot-ctrl -s a".to_string()),
        "Should switch to slot a"
    );

    let jobs = fx.execution_updates.read().await;
    let progress = jobs.first().expect("Should have at least one update");
    assert!(progress.std_out.contains("Switched from slot b to slot a"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn switches_to_other_slot_from_a() {
    let fx = JobAgentFixture::new().await;
    let shell = CommandTracker::new("a");
    fx.program().shell(shell.clone()).spawn().await;

    fx.enqueue_job(r#"slot_switch {"slot":"other"}"#)
        .await;
    fx.wait_for_updates(1).await;

    let commands = shell.commands();
    assert!(
        commands.contains(&"orb-slot-ctrl -s b".to_string()),
        "Should switch to slot b when 'other' is requested from slot a"
    );

    let jobs = fx.execution_updates.read().await;
    let progress = jobs.first().expect("Should have at least one update");
    assert!(progress.std_out.contains("Switched from slot a to slot b"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn switches_to_other_slot_from_b() {
    let fx = JobAgentFixture::new().await;
    let shell = CommandTracker::new("b");
    fx.program().shell(shell.clone()).spawn().await;

    fx.enqueue_job(r#"slot_switch {"slot":"other"}"#)
        .await;
    fx.wait_for_updates(1).await;

    let commands = shell.commands();
    assert!(
        commands.contains(&"orb-slot-ctrl -s a".to_string()),
        "Should switch to slot a when 'other' is requested from slot b"
    );

    let jobs = fx.execution_updates.read().await;
    let progress = jobs.first().expect("Should have at least one update");
    assert!(progress.std_out.contains("Switched from slot b to slot a"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn no_op_when_already_on_target_slot_a() {
    let fx = JobAgentFixture::new().await;
    let shell = CommandTracker::new("a");
    fx.program().shell(shell.clone()).spawn().await;

    fx.enqueue_job(r#"slot_switch {"slot":"a"}"#)
        .await
        .wait_for_completion()
        .await;

    // Assert: Should fail without attempting switch
    let commands = shell.commands();
    assert!(
        !commands.iter().any(|c| c.contains("orb-slot-ctrl -s")),
        "Should not attempt to switch slots"
    );
    assert!(
        !commands.iter().any(|c| c.contains("reboot")),
        "Should not attempt reboot"
    );

    let jobs = fx.execution_updates.read().await;
    let result = jobs.first().expect("Should have at least one update");
    assert_eq!(result.status, JobExecutionStatus::Failed as i32);
    assert!(result.std_err.contains("Already on slot a"));
    assert!(result.std_err.contains("nothing to do"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn no_op_when_already_on_target_slot_b() {
    let fx = JobAgentFixture::new().await;
    let shell = CommandTracker::new("b");
    fx.program().shell(shell).spawn().await;

    fx.enqueue_job(r#"slot_switch {"slot":"b"}"#)
        .await
        .wait_for_completion()
        .await;

    let jobs = fx.execution_updates.read().await;
    let result = jobs.first().expect("Should have at least one update");
    assert_eq!(result.status, JobExecutionStatus::Failed as i32);
    assert!(result.std_err.contains("Already on slot b"));
    assert!(result.std_err.contains("nothing to do"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn fails_on_invalid_slot_argument() {
    let fx = JobAgentFixture::new().await;
    let shell = CommandTracker::new("a");
    fx.program().shell(shell).spawn().await;

    fx.enqueue_job(r#"slot_switch {"slot":"c"}"#)
        .await
        .wait_for_completion()
        .await;

    let jobs = fx.execution_updates.read().await;
    let result = jobs.first().expect("Should have at least one update");
    let status = JobExecutionStatus::try_from(result.status).unwrap();
    assert_eq!(status, JobExecutionStatus::Failed);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn fails_on_missing_slot_argument() {
    let fx = JobAgentFixture::new().await;
    let shell = CommandTracker::new("a");
    fx.program().shell(shell).spawn().await;

    fx.enqueue_job(r#"slot_switch {}"#)
        .await
        .wait_for_completion()
        .await;

    let jobs = fx.execution_updates.read().await;
    let result = jobs.first().expect("Should have at least one update");
    let status = JobExecutionStatus::try_from(result.status).unwrap();
    assert_eq!(status, JobExecutionStatus::Failed);
}
