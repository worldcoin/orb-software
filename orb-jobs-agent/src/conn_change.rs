use crate::job_system::client::JobClient;
use color_eyre::{eyre::eyre, Result};
use orb_connd_events::ArchivedConnection;
use orb_info::OrbId;
use rkyv::AlignedVec;
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
        .subscriber("connd/net/changed", async |client, sample| {
            let mut bytes = AlignedVec::with_capacity(sample.payload().len());
            bytes.extend_from_slice(&sample.payload().to_bytes());
            let archived =
                rkyv::check_archived_root::<orb_connd_events::Connection>(&bytes)
                    .map_err(|e| eyre!("failed to deserialize Connection evt {e}"))?;

            match archived {
                ArchivedConnection::ConnectedGlobal(kind) => {
                    info!(
                        ?kind,
                        "detected changed in connectivity, force relay reconnection"
                    );
                }

                conn => {
                    info!(
                        ?conn,
                        "detected changed in connectivity, but we have no global connectivity. doing nothing"
                    );
                }
            };

            client.force_relay_reconnect().await?;

            Ok(())
        })
        .run()
        .await?;

    Ok(session)
}
