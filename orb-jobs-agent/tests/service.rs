use common::fixture::JobAgentFixture;
use orb_jobs_agent::shell::{Host, Shell};
use orb_relay_messages::{jobs::v1::JobExecutionStatus, tonic::async_trait};
use std::sync::{Arc, Mutex};
use tokio::process::Child;

mod common;

#[derive(Debug, Clone)]
struct MockShell {
    last_command: Arc<Mutex<Vec<String>>>,
}

impl MockShell {
    fn new() -> Self {
        Self {
            last_command: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn get_last_command(&self) -> Vec<String> {
        self.last_command.lock().unwrap().clone()
    }
}

#[async_trait]
impl Shell for MockShell {
    async fn exec(&self, cmd: &[&str]) -> color_eyre::Result<Child> {
        if cmd.first() == Some(&"systemctl") {
            // Store the command for verification
            let mut last = self.last_command.lock().unwrap();
            *last = cmd.iter().map(|s| s.to_string()).collect();

            Ok(tokio::process::Command::new("echo")
                .arg("success")
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()?)
        } else {
            Host.exec(cmd).await
        }
    }
}

#[tokio::test]
async fn it_rejects_invalid_action() {
    // Arrange
    let fx = JobAgentFixture::new().await;
    let mock_shell = MockShell::new();

    fx.program().shell(mock_shell).spawn().await;

    // Act
    fx.enqueue_job("service invalid worldcoin-core.service")
        .await
        .wait_for_completion()
        .await;

    // Assert
    let result = fx.execution_updates.read().await;
    assert_eq!(result[0].status, JobExecutionStatus::Failed as i32);

    let results = fx.execution_updates.map_iter(|x| x.std_err).await;
    assert!(results[0].contains("Invalid action"));
    assert!(results[0].contains("Must be one of: start, stop, restart, status"));
}

#[tokio::test]
async fn it_prevents_command_injection_via_semicolon() {
    // Arrange
    let fx = JobAgentFixture::new().await;
    let mock_shell = MockShell::new();
    let mock_shell_clone = mock_shell.clone();

    fx.program().shell(mock_shell).spawn().await;

    // Act
    fx.enqueue_job("service stop worldcoin-core.service;shutdown")
        .await
        .wait_for_completion()
        .await;

    // Assert - the entire string should be passed as a single argument to systemctl
    let last_cmd = mock_shell_clone.get_last_command();
    assert_eq!(
        last_cmd,
        vec!["systemctl", "stop", "worldcoin-core.service;shutdown"]
    );
    // systemctl will fail with "no such service" but no command injection occurs
}

#[tokio::test]
async fn it_prevents_command_injection_via_pipe() {
    // Arrange
    let fx = JobAgentFixture::new().await;
    let mock_shell = MockShell::new();
    let mock_shell_clone = mock_shell.clone();

    fx.program().shell(mock_shell).spawn().await;

    // Act
    fx.enqueue_job("service stop worldcoin-core.service|cat")
        .await
        .wait_for_completion()
        .await;

    // Assert - the entire string should be passed as a single argument
    let last_cmd = mock_shell_clone.get_last_command();
    assert_eq!(
        last_cmd,
        vec!["systemctl", "stop", "worldcoin-core.service|cat"]
    );
}

#[tokio::test]
async fn it_prevents_command_injection_via_ampersand() {
    // Arrange
    let fx = JobAgentFixture::new().await;
    let mock_shell = MockShell::new();
    let mock_shell_clone = mock_shell.clone();

    fx.program().shell(mock_shell).spawn().await;

    // Act
    fx.enqueue_job("service stop worldcoin-core.service&whoami")
        .await
        .wait_for_completion()
        .await;

    // Assert - the entire string should be passed as a single argument
    let last_cmd = mock_shell_clone.get_last_command();
    assert_eq!(
        last_cmd,
        vec!["systemctl", "stop", "worldcoin-core.service&whoami"]
    );
}
