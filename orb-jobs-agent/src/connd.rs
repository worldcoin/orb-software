use color_eyre::{eyre::bail, Result};
use orb_connd_dbus::{AccessPoint, ConndProxy, ConnectionState};
use std::time::{Duration, Instant};
use tokio::time;
use tracing::{info, warn};

pub async fn connect_to_wifi_and_wait_for_internet(
    connd: &ConndProxy<'_>,
    ssid: impl Into<String>,
    timeout: Duration,
) -> Result<AccessPoint> {
    let ssid = ssid.into();

    let wifi_conn_start = Instant::now();
    let res = connd.connect_to_wifi(ssid.clone()).await?;
    let time_to_wifi_s = wifi_conn_start.elapsed().as_secs();

    info!(
        ssid,
        time_to_wifi_s, "took {time_to_wifi_s}s to connect to {ssid}"
    );

    let internet_conn_start = Instant::now();
    match wait_for_internet(connd, timeout).await {
        Err(e) => warn!(ssid, "failed waiting for internet: {e:?}"),
        Ok(_) => {
            let time_to_internet_s = internet_conn_start.elapsed().as_secs();
            info!(
                ssid,
                time_to_internet_s,
                "took {time_to_internet_s}s to connect to the internet after joining network"
            );
        }
    }

    Ok(res)
}

async fn wait_for_internet(connd: &ConndProxy<'_>, timeout: Duration) -> Result<()> {
    let now = Instant::now();

    while now.elapsed() < timeout {
        use ConnectionState::*;
        match connd.connection_state().await? {
            Connected => return Ok(()),
            Connecting | PartiallyConnected => {
                time::sleep(Duration::from_secs(1)).await
            }

            _ => break,
        }
    }

    bail!("failed to establish connection")
}
