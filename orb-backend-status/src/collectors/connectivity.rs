use super::{GlobalConnectivity, ZenorbCtx};
use color_eyre::Result;
use tracing::info;
use zenorb::zenoh::sample::Sample;

pub(crate) async fn handle_connection_event(
    ctx: ZenorbCtx,
    sample: Sample,
) -> Result<()> {
    let payload = sample.payload().to_bytes();
    let active_conns: oes::ActiveConnections = serde_json::from_slice(&payload)?;

    let connected = active_conns.connections.iter().any(|c| c.has_internet);
    let ssid = active_conns
        .connections
        .into_iter()
        .find(|c| c.iface == oes::NetworkInterface::WiFi && c.has_internet)
        .map(|c| c.name);

    let connectivity = if connected {
        GlobalConnectivity::Connected { ssid }
    } else {
        GlobalConnectivity::NotConnected
    };

    let prev = ctx.connectivity_tx.borrow().clone();
    if prev != connectivity {
        info!("global connectivity changed: {connectivity:?}");

        let prev_ssid = prev.ssid();
        let new_ssid = connectivity.ssid();
        if prev_ssid != new_ssid {
            ctx.backend_status
                .update_active_ssid(new_ssid.map(String::from));
            ctx.backend_status.set_send_immediately();
        }
    }

    ctx.connectivity_tx
        .send(connectivity)
        .map_err(|e| color_eyre::eyre::eyre!("{e}"))?;

    Ok(())
}
