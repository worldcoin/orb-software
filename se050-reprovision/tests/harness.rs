use std::time::Duration;

use bytes::Bytes;
use orb_se050_reprovision::cli::{CliStrategy, MockChild};
use rand::SeedableRng;
use tempfile::TempDir;

#[derive(Debug, bon::Builder)]
pub struct Harness {
    #[builder(default = TempDir::new().expect("failed to create tempdir"))]
    tempdir: tempfile::TempDir,
    seed: u64,
    #[builder(default = Duration::from_millis(5000))]
    timeout: Duration,
    mocked_server: wiremock::MockServer,
    #[builder(skip)]
    pub mocked_cli: CliProxy,
}

impl Harness {
    pub fn make_program_cfg(&self) -> orb_se050_reprovision::Config {
        orb_se050_reprovision::Config {
            rng: rand::rngs::StdRng::seed_from_u64(self.seed),
            client: orb_se050_reprovision::remote_api::Client::builder()
                .local_backend(self.mocked_server.address().port())
                .custom_reqwest_client(reqwest::Client::new())
                .build(),
            ca_config: CliStrategy::Mocked(self.mocked_cli.inner.clone()),
        }
    }
}

#[derive(Debug)]
pub struct CliProxy {
    pub stdout: flume::Sender<Bytes>,
    pub stdin: flume::Receiver<Bytes>,
    inner: MockChild,
}

impl Default for CliProxy {
    fn default() -> Self {
        let (stdout_tx, stdout_rx) = flume::unbounded();
        let (stdin_tx, stdin_rx) = flume::unbounded();
        Self {
            stdout: stdout_tx,
            stdin: stdin_rx,
            inner: MockChild {
                stdin: stdin_tx,
                stdout: stdout_rx,
            },
        }
    }
}
