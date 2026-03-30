use crate::job_system::client::JobClient;
use color_eyre::{eyre::eyre, Result};
use orb_info::OrbId;
use tokio::task;
use tracing::{info, warn};
use zenorb::Zenorb;

/// forces relay reconnection every time there is a change to connectivity
pub async fn spawn_watcher(
    orb_id: OrbId,
    client: JobClient,
    zenoh_port: u16,
) -> Result<Zenorb> {
    info!("setting up zenoh subscribers");
    let session = Zenorb::from_cfg(zenorb::client_cfg(zenoh_port))
        .orb_id(orb_id)
        .with_name("jobs-agent")
        .await?;

    let sub = session
        .declare_subscriber("connd/oes/active_connections")
        .await
        .map_err(|e| {
            eyre!("failed to subscribe to connd/oes/active_connections: {e}")
        })?;

    task::spawn(async move {
        let mut state = (false, None);

        loop {
            let sample = sub.recv_async().await.map_err(|e| {
                eyre!(
                    "failed to receive message from connd/oes/active_connections: {e}"
                )
            })?;

            let active_conns: oes::ActiveConnections =
                serde_json::from_slice(&sample.payload().to_bytes())?;

            let is_online = active_conns.connections.iter().any(|c| c.has_internet);
            let primary = active_conns
                .connections
                .iter()
                .find(|c| c.primary)
                .map(|c| &c.name);

            let changed = (is_online != state.0) || (primary != state.1.as_ref());
            state.0 = is_online;
            state.1 = primary.cloned();

            if !changed {
                continue;
            }

            match (is_online, primary) {
                (true, Some(con)) => {
                    warn!("new primary connection: {con}, forcing relay reconnection");
                    client.force_relay_reconnect().await?;
                }

                (true, None) => {
                    warn!("detected changed in connectivity, but we have global connectivity but no primary connection. doing nothing");
                }

                (false, _) => {
                    warn!("detected changed in connectivity, but we have no global connectivity. doing nothing");
                }
            }
        }

        #[allow(unreachable_code)]
        Ok::<(), color_eyre::Report>(())
    });

    Ok(session)
}
