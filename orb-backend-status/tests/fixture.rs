use async_tempfile::TempDir;
use color_eyre::Result;
use dbus_launch::BusType;
use orb_info::{OrbId, OrbJabilId, OrbName};
use reqwest::Url;
use std::{env, path::PathBuf, str::FromStr, time::Duration};
use tokio::{
    task::{self, JoinHandle},
    time,
};
use tokio_util::sync::CancellationToken;
use wiremock::MockServer;

pub struct Fixture {
    _dbusd: dbus_launch::Daemon,
    _tmpdir: TempDir,
    pub dbus: zbus::Connection,
    endpoint: Url,
    orb_os_version: String,
    orb_id: OrbId,
    orb_name: OrbName,
    orb_jabil_id: OrbJabilId,
    procfs: PathBuf,
    netstats_poll_interval: Duration,
    sender_interval: Duration,
    sender_min_backoff: Duration,
    sender_max_backoff: Duration,
    req_timeout: Duration,
    req_min_retry_interval: Duration,
    req_max_retry_interval: Duration,
    shutdown_token: CancellationToken,
    pub mock_server: MockServer,
}

#[bon::bon]
impl Fixture {
    pub async fn new() -> Self {
        Fixture::with().build().await
    }

    #[builder(start_fn = with)]
    pub async fn builder(
        #[builder(default = Duration::from_secs(30))] netstats_poll_interval: Duration,
        #[builder(default = Duration::from_secs(30))] sender_interval: Duration,
        #[builder(default = Duration::from_secs(1))] sender_min_backoff: Duration,
        #[builder(default = Duration::from_secs(30))] sender_max_backoff: Duration,
        #[builder(default = Duration::from_secs(5))] req_timeout: Duration,
        #[builder(default = Duration::from_millis(100))]
        req_min_retry_interval: Duration,
        #[builder(default = Duration::from_secs(2))] req_max_retry_interval: Duration,
    ) -> Self {
        let shutdown_token = CancellationToken::new();
        let mock_server = MockServer::start().await;

        let dbusd = tokio::task::spawn_blocking(|| {
            dbus_launch::Launcher::daemon()
                .bus_type(BusType::Session)
                .launch()
                .expect("failed to launch dbus-daemon")
        })
        .await
        .expect("task panicked");

        let dbus = zbus::ConnectionBuilder::address(dbusd.address())
            .unwrap()
            .build()
            .await
            .unwrap();

        let tmpdir = TempDir::new().await.unwrap();
        let procfs = tmpdir.to_path_buf();

        let endpoint = mock_server.address().to_string();
        let endpoint = format!("http://{endpoint}").parse().unwrap();

        Fixture {
            _tmpdir: tmpdir,
            _dbusd: dbusd,
            dbus,
            endpoint,
            orb_os_version: "6.6.6".into(),
            orb_id: OrbId::from_str("bba85baa").unwrap(),
            orb_name: OrbName("ota-hilly".into()),
            orb_jabil_id: OrbJabilId("straighttojail".into()),
            procfs,
            mock_server,
            netstats_poll_interval,
            sender_interval,
            sender_min_backoff,
            sender_max_backoff,
            req_timeout,
            req_min_retry_interval,
            req_max_retry_interval,
            shutdown_token,
        }
    }

    pub async fn start(&self) -> JoinHandle<Result<()>> {
        let program = orb_backend_status::program()
            .dbus(self.dbus.clone())
            .endpoint(self.endpoint.clone())
            .orb_os_version(self.orb_os_version.clone())
            .orb_id(self.orb_id.clone())
            .orb_name(self.orb_name.clone())
            .orb_jabil_id(self.orb_jabil_id.clone())
            .procfs(self.procfs.clone())
            .net_stats_poll_interval(self.netstats_poll_interval)
            .sender_interval(self.sender_interval)
            .sender_min_backoff(self.sender_min_backoff)
            .sender_max_backoff(self.sender_max_backoff)
            .req_timeout(self.req_timeout)
            .req_min_retry_interval(self.req_min_retry_interval)
            .req_max_retry_interval(self.req_max_retry_interval)
            .shutdown_token(self.shutdown_token.clone());

        let task = task::spawn(async move {
            program
                .run()
                .await
                .inspect_err(|e| println!("program failed: {e}"))
        });

        let secs = if env::var("GITHUB_ACTIONS").is_ok() {
            5
        } else {
            1
        };

        time::sleep(Duration::from_secs(secs)).await;

        task
    }

    pub fn stop(&self) {
        self.shutdown_token.cancel();
    }

    pub fn log(&self) -> &Self {
        let _ = orb_telemetry::TelemetryConfig::new().init();
        self
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop();
    }
}
