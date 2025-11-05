use std::{
    process::Stdio,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use async_trait::async_trait;
use color_eyre::Result;
use common::fixture::JobAgentFixture;
use orb_jobs_agent::shell::Shell;
use orb_relay_messages::jobs::v1::JobExecutionStatus;
use tokio::{process::Child, time};

mod common;

#[cfg_attr(target_os = "macos", test_with::no_env(GITHUB_ACTIONS))]
#[tokio::test]
async fn it_shuts_orb_down() {
    // Arrange
    let ms = MockShell::default();
    let fx = JobAgentFixture::new().await;
    fx.program().shell(ms.clone()).spawn().await;

    // Act
    fx.enqueue_job("shutdown").await.wait_for_completion().await;

    // Assert
    let result = fx.execution_updates.read().await;
    assert_eq!(result[0].status, JobExecutionStatus::Succeeded as i32);

    time::sleep(Duration::from_secs(5)).await;
    let shutdown_called = ms.shutdown_called.load(Ordering::SeqCst);
    assert!(shutdown_called);
}

#[derive(Clone, Debug, Default)]
struct MockShell {
    shutdown_called: Arc<AtomicBool>,
}

#[async_trait]
impl Shell for MockShell {
    async fn exec(&self, cmd: &[&str]) -> Result<Child> {
        if cmd == ["shutdown", "now"] {
            self.shutdown_called.store(true, Ordering::SeqCst);
        }

        let child = tokio::process::Command::new("true")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        Ok(child)
    }
}
