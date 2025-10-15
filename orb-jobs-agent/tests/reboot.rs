use color_eyre::Result;
use common::{fake_orb::FakeOrb, fixture::JobAgentFixture};
use orb_jobs_agent::shell::Shell;
use orb_relay_messages::jobs::v1::JobExecutionStatus;
use std::{sync::Arc, time::Duration};
use tokio::{fs, sync::Mutex, time};

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
    let ticket = fx.enqueue_job("reboot").await;
    time::sleep(Duration::from_secs(1)).await;

    // Assert
    let jobs = fx.execution_updates.read().await;
    let actual = jobs.first().unwrap();

    let pending_execution_id = fs::read_to_string(&reboot_lockfile).await.unwrap();

    assert_eq!(ticket.exec_id, pending_execution_id);
    assert_eq!(actual.std_out, "rebooting\n");
    assert_eq!(actual.status, JobExecutionStatus::InProgress as i32);

    // 2. Simulate Orb Reboot
    program_handle.stop().await;
    fx.spawn_program(FakeOrb::new().await);

    // 3. Receive command from backend, finish execution -- lockfile should be removed
    fx.enqueue_job_with_id("reboot", ticket.exec_id)
        .await
        .wait_for_completion()
        .await;

    // Assert
    let jobs = fx.execution_updates.read().await;
    let last_progress = &jobs[jobs.len() - 2];
    let success = &jobs[jobs.len() - 1];

    assert!(!fs::try_exists(reboot_lockfile).await.unwrap());
    assert_eq!(success.status, JobExecutionStatus::Succeeded as i32);
    assert_eq!(last_progress.status, JobExecutionStatus::InProgress as i32);
    assert_eq!(last_progress.std_out, "rebooted");
}

#[tokio::test]
async fn reboot_commands_are_executed_after_lockfile() {
    // This test verifies the critical ordering issue mentioned in the code review:
    // The reboot commands (orb-mcu-util + shutdown) must be executed AFTER
    // the lockfile is written and progress is sent.
    
    #[derive(Clone, Debug)]
    struct CommandTracker {
        commands: Arc<Mutex<Vec<String>>>,
        lockfile_path: Arc<Mutex<Option<String>>>,
    }
    
    impl CommandTracker {
        fn new() -> Self {
            Self {
                commands: Arc::new(Mutex::new(Vec::new())),
                lockfile_path: Arc::new(Mutex::new(None)),
            }
        }
        
        async fn set_lockfile_path(&self, path: String) {
            *self.lockfile_path.lock().await = Some(path);
        }
        
        async fn commands(&self) -> Vec<String> {
            self.commands.lock().await.clone()
        }
    }
    
    #[async_trait::async_trait]
    impl Shell for CommandTracker {
        async fn exec(&self, cmd: &[&str]) -> Result<tokio::process::Child> {
            let cmd_str = cmd.join(" ");
            
            // Before executing reboot commands, check if lockfile exists
            if cmd_str.contains("orb-mcu-util") && cmd_str.contains("reboot") {
                if let Some(ref lockfile) = *self.lockfile_path.lock().await {
                    let exists = fs::try_exists(lockfile).await.unwrap_or(false);
                    self.commands.lock().await.push(
                        format!("lockfile_exists={exists} before {cmd_str}")
                    );
                }
            }
            
            self.commands.lock().await.push(cmd_str);
            
            // Simulate success
            Ok(tokio::process::Command::new("true")
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()?)
        }
    }
    
    let fx = JobAgentFixture::new().await;
    let tracker = CommandTracker::new();
    let reboot_lockfile = fx.settings.store_path.join("reboot.lock");
    tracker.set_lockfile_path(reboot_lockfile.to_string_lossy().to_string()).await;
    
    let _program_handle = fx.spawn_program(tracker.clone());
    
    // Execute reboot command
    fx.enqueue_job("reboot").await;
    time::sleep(Duration::from_secs(1)).await;
    
    // Verify reboot commands were executed
    let commands = tracker.commands().await;
    
    // Find the reboot commands
    let has_mcu_reboot = commands.iter().any(|cmd| {
        cmd.contains("orb-mcu-util") && cmd.contains("reboot") && cmd.contains("orb")
    });
    let has_shutdown = commands.iter().any(|cmd| {
        cmd.contains("shutdown") && cmd.contains("now")
    });
    
    assert!(has_mcu_reboot, "Should call orb-mcu-util reboot. Commands: {commands:?}");
    assert!(has_shutdown, "Should call shutdown now. Commands: {commands:?}");
    
    // Verify order: mcu-util before shutdown
    let mcu_idx = commands.iter().position(|cmd| cmd.contains("orb-mcu-util")).unwrap();
    let shutdown_idx = commands.iter().position(|cmd| cmd.contains("shutdown")).unwrap();
    assert!(mcu_idx < shutdown_idx, "orb-mcu-util should be called before shutdown");
    
    // CRITICAL: Verify lockfile existed when reboot commands were executed
    let lockfile_check = commands.iter().find(|cmd| {
        cmd.contains("lockfile_exists") && cmd.contains("orb-mcu-util")
    });
    
    assert!(
        lockfile_check.is_some() && lockfile_check.unwrap().contains("lockfile_exists=true"),
        "Lockfile should exist before reboot commands are executed. Commands: {commands:?}"
    );
}
