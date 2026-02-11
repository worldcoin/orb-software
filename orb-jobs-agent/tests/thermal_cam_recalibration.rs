use color_eyre::Result;
use common::fixture::JobAgentFixture;
use orb_jobs_agent::shell::Shell;
use orb_relay_messages::jobs::v1::JobExecutionStatus;
use std::{
    collections::HashSet,
    process::Stdio,
    sync::{Arc, Mutex},
};
use tokio::process::{Child, Command};

mod common;

const STOP_CMD: &str = "systemctl stop worldcoin-core.service";
const CALIBRATION_CMD: &str =
    "/usr/bin/env SEEKTHERMAL_ROOT=/usr/persistent /usr/bin/orb-thermal-cam-ctrl calibration fsc";
const START_CMD: &str = "systemctl start worldcoin-core.service";

#[derive(Clone, Debug)]
struct RecordingShell {
    commands: Arc<Mutex<Vec<String>>>,
    commands_to_fail: Arc<HashSet<String>>,
}

impl RecordingShell {
    fn new(commands_to_fail: HashSet<String>) -> Self {
        Self {
            commands: Arc::new(Mutex::new(Vec::new())),
            commands_to_fail: Arc::new(commands_to_fail),
        }
    }

    fn commands(&self) -> Vec<String> {
        self.commands.lock().expect("mutex poisoned").clone()
    }
}

#[async_trait::async_trait]
impl Shell for RecordingShell {
    async fn exec(&self, cmd: &[&str]) -> Result<Child> {
        let command = cmd.join(" ");
        self.commands
            .lock()
            .expect("mutex poisoned")
            .push(command.clone());

        let mut process = if self.commands_to_fail.contains(&command) {
            let mut cmd = Command::new("sh");
            cmd.args(["-c", "echo mocked failure >&2; exit 1"]);
            cmd
        } else {
            Command::new("true")
        };

        Ok(process
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn thermal_cam_recalibration_runs_commands_in_order() {
    let fx = JobAgentFixture::new().await;
    let shell = RecordingShell::new(HashSet::new());
    fx.program().shell(shell.clone()).spawn().await;

    fx.enqueue_job("thermal_cam_recalibration")
        .await
        .wait_for_completion()
        .await;

    let jobs = fx.execution_updates.read().await;
    let result = jobs.last().unwrap();

    assert_eq!(result.status, JobExecutionStatus::Succeeded as i32);
    assert_eq!(
        shell.commands(),
        vec![
            STOP_CMD.to_string(),
            CALIBRATION_CMD.to_string(),
            START_CMD.to_string()
        ]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn thermal_cam_recalibration_restarts_core_even_if_calibration_fails() {
    let fx = JobAgentFixture::new().await;
    let commands_to_fail = HashSet::from([CALIBRATION_CMD.to_string()]);
    let shell = RecordingShell::new(commands_to_fail);
    fx.program().shell(shell.clone()).spawn().await;

    fx.enqueue_job("thermal_cam_recalibration")
        .await
        .wait_for_completion()
        .await;

    let jobs = fx.execution_updates.read().await;
    let result = jobs.last().unwrap();

    assert_eq!(result.status, JobExecutionStatus::Failed as i32);
    assert_eq!(
        shell.commands(),
        vec![
            STOP_CMD.to_string(),
            CALIBRATION_CMD.to_string(),
            START_CMD.to_string()
        ]
    );
    assert!(result
        .std_err
        .contains("running thermal camera calibration failed"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn thermal_cam_recalibration_starts_core_even_if_stop_fails() {
    let fx = JobAgentFixture::new().await;
    let commands_to_fail = HashSet::from([STOP_CMD.to_string()]);
    let shell = RecordingShell::new(commands_to_fail);
    fx.program().shell(shell.clone()).spawn().await;

    fx.enqueue_job("thermal_cam_recalibration")
        .await
        .wait_for_completion()
        .await;

    let jobs = fx.execution_updates.read().await;
    let result = jobs.last().unwrap();

    assert_eq!(result.status, JobExecutionStatus::Failed as i32);
    assert_eq!(
        shell.commands(),
        vec![STOP_CMD.to_string(), START_CMD.to_string()]
    );
    assert!(result
        .std_err
        .contains("stopping worldcoin-core.service failed"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn thermal_cam_recalibration_reports_both_errors_if_stop_and_start_fail() {
    let fx = JobAgentFixture::new().await;
    let commands_to_fail = HashSet::from([STOP_CMD.to_string(), START_CMD.to_string()]);
    let shell = RecordingShell::new(commands_to_fail);
    fx.program().shell(shell.clone()).spawn().await;

    fx.enqueue_job("thermal_cam_recalibration")
        .await
        .wait_for_completion()
        .await;

    let jobs = fx.execution_updates.read().await;
    let result = jobs.last().unwrap();

    assert_eq!(result.status, JobExecutionStatus::Failed as i32);
    assert_eq!(
        shell.commands(),
        vec![STOP_CMD.to_string(), START_CMD.to_string()]
    );
    assert!(result
        .std_err
        .contains("additionally failed to start worldcoin-core.service"));
}
