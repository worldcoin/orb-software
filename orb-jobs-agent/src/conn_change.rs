use crate::job_system::client::JobClient;
use color_eyre::Result;
use orb_info::OrbId;
use tracing::info;

/// forces relay reconnection every time there is a change to connectivity
pub async fn spawn_watcher(
    orb_id: OrbId,
    client: JobClient,
    zenoh_port: u16,
) -> Result<zenorb::Session> {
    let session = zenorb::Session::from_cfg(zenorb::client_cfg(zenoh_port))
        .orb_id(orb_id)
        .with_name("jobs-agent")
        .await?;

    session
        .receiver(client)
        .subscriber("connd/net/changed", async |client, _| {
            info!("detected changed in connectivity, force relay reconnection");
            client.force_relay_reconnect().await?;

            Ok(())
        })
        .run()
        .await?;

    Ok(session)
}
