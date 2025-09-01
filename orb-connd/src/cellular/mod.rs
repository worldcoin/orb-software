use crate::{
    modem_manager, telemetry::connection_state::ConnectionState, utils::run_cmd,
};
use color_eyre::{
    eyre::{bail, ContextCompat},
    Result,
};
use std::time::Duration;
use tokio::{
    task::{self, JoinHandle},
    time,
};
use tracing::{error, info, warn};

const IFACE: &str = "wwan0";
const METRIC: &str = "400";

pub async fn start(
    conn_check_interval: Duration,
    reconnect_backoff: Duration,
) -> JoinHandle<Result<()>> {
    task::spawn(async move {
        info!("starting cellular connectivity task");

        loop {
            if let Err(e) = ensure_connectivity(conn_check_interval).await {
                error!(
                    "failed to maintain cellular connecitivty, trying again in {}s. error: {e}",
                    reconnect_backoff.as_secs()
                );
            }

            time::sleep(reconnect_backoff).await;
        }
    })
}

/// this function will be called every time we (re)try to establish connectivity.
/// it always checks for modem id in case modem has power cycled or changed id for whatever reason.
async fn ensure_connectivity(conn_check_interval: Duration) -> Result<()> {
    let modem_id = modem_manager::get_modem_id().await?;

    info!("trying to establish a cellular connection. modem_id: {modem_id}");
    modem_manager::simple_connect(&modem_id, Duration::from_secs(300)).await?;
    info!("successfully established a cellular connection");

    let modem_info = modem_manager::modem_info(&modem_id).await?;
    let bearer_id = get_bearer_id(&modem_info).wrap_err_with(|| {
        format!("failed to get bearer id from mmcli. modem_id: {modem_id}. json: {modem_info}")
    })?;

    let bearer_info = modem_manager::bearer_info(bearer_id).await?;
    let mut ipv4_config = get_ipv4_config(&bearer_info).wrap_err_with(|| {
        format!("failed to get ipv4 config from mmcli. bearer_id: {bearer_id}. json: {modem_info}")
    })?;

    info!("configuring {IFACE}");
    configure_wwan_iface(&mut ipv4_config, IFACE, METRIC).await?;

    info!("finished setting up cellular connectivity and {IFACE}");

    loop {
        time::sleep(conn_check_interval).await;

        // no handling of airplane, but hopefully by the time we do we
        // have already moved to network manager
        match modem_manager::get_connection_state(&modem_id).await? {
            ConnectionState::Connected => info!("cellular connection check succeeded"),
            cs => {
                error!("lost connection, tearing down connected bearers");
                if let Err(e) = modem_manager::simple_disconnect(&modem_id).await {
                    warn!("failed to call simple_disconnect on modem_id {modem_id}. err: {e}");
                }

                bail!(
                    "expected connection state to be 'Connected', instead got '{cs:?}'"
                );
            }
        }
    }
}

fn get_bearer_id(modem_info: &serde_json::Value) -> Option<usize> {
    modem_info
        .get("modem")?
        .get("generic")?
        .get("bearers")?
        .as_array()?
        .first()?
        .as_str()?
        .split("/")
        .last()?
        .parse()
        .ok()
}

fn get_ipv4_config(bearer_info: &serde_json::Value) -> Option<Ipv4Config> {
    let cfg = bearer_info.get("bearer")?.get("ipv4-config")?;

    Some(Ipv4Config {
        ip: cfg.get("address")?.as_str()?,
        prefix: cfg.get("prefix")?.as_str()?,
        gateway: cfg.get("gateway")?.as_str()?,
    })
}

struct Ipv4Config<'a> {
    ip: &'a str,
    prefix: &'a str,
    gateway: &'a str,
}

async fn configure_wwan_iface(
    cfg: &mut Ipv4Config<'_>,
    wwan_iface: &str,
    metric: &str,
) -> Result<()> {
    run_cmd("ip", &["link", "set", wwan_iface, "up"]).await?;
    run_cmd("ip", &["addr", "flush", "dev", wwan_iface]).await?;
    run_cmd(
        "ip",
        &[
            "addr",
            "replace",
            &format!("{}/{}", cfg.ip, cfg.prefix),
            "dev",
            wwan_iface,
        ],
    )
    .await?;
    run_cmd(
        "ip",
        &[
            "route",
            "replace",
            "default",
            "via",
            cfg.gateway,
            "dev",
            wwan_iface,
            "metric",
            metric,
        ],
    )
    .await?;

    Ok(())
}
