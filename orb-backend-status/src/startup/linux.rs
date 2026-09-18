use super::Config;
use color_eyre::Result;
use orb_backend_status::collectors;
use orb_endpoints::{v2::Endpoints, Backend};
use orb_info::{orb_os_release::OrbOsRelease, OrbId, OrbJabilId, OrbName};
use reqwest::Url;
use std::time::Duration;
use tracing::warn;

pub async fn configure() -> Result<Config> {
    let orb_id = OrbId::read().await?;
    let endpoint = Endpoints::new(Backend::from_env()?, &orb_id).status;
    let endpoint = Url::parse(endpoint.as_str())?;

    let orb_name = OrbName::read().await.unwrap_or_else(|e| {
        warn!("failed to read orb name: {e:?}");
        OrbName("unknown".to_string())
    });
    let orb_jabil_id = OrbJabilId::read().await.unwrap_or_else(|e| {
        warn!("failed to read orb jabil id: {e:?}");
        OrbJabilId("unknown".to_string())
    });

    Ok(Config {
        orb_id,
        orb_name,
        orb_jabil_id,
        orb_os_version: OrbOsRelease::read().await?.platform_version(),
        endpoint,
        zenoh: zenorb::default_cfg(),
        collectors: collectors::Config {
            dbus: zbus::Connection::session().await?,
            net_stats_poll_interval: Duration::from_secs(30),
        },
        metrics_socket: None,
    })
}
