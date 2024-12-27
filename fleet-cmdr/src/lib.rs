pub mod args;
pub mod orb_info;
pub mod settings;

use color_eyre::eyre::{eyre, Result};
use orb_build_info::{make_build_info, BuildInfo};
use orb_endpoints::{Endpoints, OrbId};
use orb_relay_client::client::Client;
use orb_relay_messages::common;
use std::time::Duration;
use tracing::error;

pub const BUILD_INFO: BuildInfo = make_build_info!();

const ORB_FLEET_CMDR_NAMESPACE: &str = "orb-fleet-cmdr";
const ORB_RELAY_DEST_ID: &str = "orb-fleet-cmdr";

pub async fn relay_connect(
    orb_id: &OrbId,
    orb_token: String,
    endpoints: &Endpoints,
    reties: u32,
    timeout: Duration,
) -> Result<Client> {
    let mut relay = Client::new_as_orb(
        endpoints.relay.to_string(),
        orb_token,
        orb_id.to_string(),
        ORB_RELAY_DEST_ID.to_string(),
        ORB_FLEET_CMDR_NAMESPACE.to_string(),
    );
    if let Err(e) = relay.connect().await {
        return Err(eyre!("Relay: Failed to connect: {e}"));
    }
    for _ in 0..reties {
        if let Ok(()) = relay
            .send_blocking(
                common::v1::AnnounceOrbId {
                    orb_id: orb_id.to_string(),
                    mode_type: common::v1::announce_orb_id::ModeType::SelfServe.into(),
                    hardware_type: common::v1::announce_orb_id::HardwareType::Pearl
                        .into(),
                },
                timeout,
            )
            .await
        {
            // Happy path. We have successfully announced and acknowledged the OrbId.
            return Ok(relay);
        }
        error!("Relay: Failed to AnnounceOrbId. Retrying...");
        relay.reconnect().await?;
        if relay.has_pending_messages().await? > 0 {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
    Err(eyre!(
        "Relay: Failed to send AnnounceOrbId after a reconnect"
    ))
}

pub async fn relay_disconnect(
    relay: &mut Client,
    wait_for_pending_messages: Duration,
    wait_for_shutdown: Duration,
) -> Result<()> {
    relay
        .graceful_shutdown(wait_for_pending_messages, wait_for_shutdown)
        .await;
    Ok(())
}
