use color_eyre::eyre::Result;
use orb_dogd::DogstatsdClient;
use orb_endpoints::{v2::Endpoints, Backend};
use orb_info::{orb_os_release::OrbOsRelease, OrbId, OrbJabilId, OrbName};
use reqwest::Url;
use signal_hook::consts::{SIGINT, SIGTERM};
use std::time::Duration;
use tracing::warn;

const SYSLOG_IDENTIFIER: &str = "worldcoin-oes";

fn main() -> Result<()> {
    color_eyre::install()?;
    let telemetry = orb_telemetry::TelemetryConfig::new()
        .with_journald(SYSLOG_IDENTIFIER)
        .init();

    let (shutdown_tx, shutdown_rx) = flume::unbounded::<()>();
    std::thread::spawn(move || {
        let mut signals = signal_hook::iterator::Signals::new([SIGTERM, SIGINT])
            .expect("failed to register signal handler");

        if signals.forever().next().is_some() {
            warn!("received shutdown signal");
        }

        drop(shutdown_tx);
    });

    let orb_id = OrbId::read_blocking()?;
    let endpoint = Endpoints::new(Backend::from_env()?, &orb_id).status;
    let endpoint = Url::parse(endpoint.as_str())?;

    let orb_name = OrbName::read_blocking().unwrap_or_else(|e| {
        warn!("failed to read orb name: {e:?}");
        OrbName("unknown".to_string())
    });
    let orb_jabil_id = OrbJabilId::read_blocking().unwrap_or_else(|e| {
        warn!("failed to read orb jabil id: {e:?}");
        OrbJabilId("unknown".to_string())
    });

    let result = orb_oes::program()
        .metrics(DogstatsdClient::default())
        .endpoint(endpoint)
        .orb_os_version(OrbOsRelease::read_blocking()?.platform_version())
        .orb_id(orb_id)
        .orb_name(orb_name)
        .orb_jabil_id(orb_jabil_id)
        .procfs("/proc")
        .sender_interval(Duration::from_secs(30))
        .req_timeout(Duration::from_secs(2))
        .req_min_retry_interval(Duration::from_millis(100))
        .req_max_retry_interval(Duration::from_secs(500))
        .shutdown_rx(shutdown_rx)
        .run();

    telemetry.flush_blocking();

    result
}
