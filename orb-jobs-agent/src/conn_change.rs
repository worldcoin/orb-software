use crate::job_system::client::JobClient;
use color_eyre::Result;
use orb_info::OrbId;
use tracing::info;
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

    session
        .receiver(client)
        .subscriber("connd/oes/active_connections", async |client, sample| {
            let active_conns: oes::ActiveConnections = serde_json::from_slice(&sample.payload().to_bytes())?;
            let is_online = active_conns.connections.iter().any(|c|c.has_internet);
            let primary = active_conns.connections.iter().find(|c|c.primary).map(|c|&c.name);

            if !is_online {
                info!("detected changed in connectivity, but we have no global connectivity. doing nothing");
                return Ok(())
            }

            info!("new primary connection: {primary:?}, forcing relay reconnection");
            client.force_relay_reconnect().await?;

            Ok(())
        })
        .run()
        .await?;

    Ok(session)
}
