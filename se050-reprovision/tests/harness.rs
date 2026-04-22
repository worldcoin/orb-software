use bytes::Bytes;
use orb_se050_reprovision::cli::{CliStrategy, MockChild};
use rand::SeedableRng;
use wiremock::MockServer;

use self::harness_builder as hb;

#[derive(Debug, bon::Builder)]
#[builder(finish_fn(name = build_inner, vis = ""))]
pub struct Harness {
    #[builder(finish_fn)]
    pub cli_proxy: CliProxy,
    seed: u64,
    mocked_server: MockServer,
}

impl<S: hb::State> HarnessBuilder<S> {
    pub fn build(self) -> (Harness, orb_se050_reprovision::Config)
    where
        S: hb::IsComplete,
    {
        let (cli_proxy, mock_child) = CliProxy::new();
        let harness = self.build_inner(cli_proxy);
        let program_cfg = orb_se050_reprovision::Config {
            rng: rand::rngs::StdRng::seed_from_u64(harness.seed),
            client: orb_se050_reprovision::remote_api::Client::builder()
                .local_backend(harness.mocked_server.address().port())
                .custom_reqwest_client(reqwest::Client::new())
                .build(),
            cli_strat: CliStrategy::Mocked(mock_child),
        };

        (harness, program_cfg)
    }
}

#[derive(Debug)]
pub struct CliProxy {
    pub stdout: flume::Sender<Bytes>,
    pub stdin: flume::Receiver<Bytes>,
}

impl CliProxy {
    fn new() -> (Self, MockChild) {
        let (stdout_tx, stdout_rx) = flume::unbounded();
        let (stdin_tx, stdin_rx) = flume::unbounded();
        let mock_child = MockChild {
            stdin: stdin_tx,
            stdout: stdout_rx,
        };
        (
            Self {
                stdout: stdout_tx,
                stdin: stdin_rx,
            },
            mock_child,
        )
    }
}
