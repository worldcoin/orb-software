use color_eyre::{eyre::eyre, Result};
use eyre::Context;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::task;
use tracing::error;

pub async fn spawn_watcher(
    zenorb: &zenorb::Zenorb,
    is_online: Arc<AtomicBool>,
) -> Result<()> {
    let sub = zenorb
        .declare_subscriber("connd/oes/active_connections")
        .await
        .map_err(|e| {
            eyre!("failed to subscribe to connd/oes/active_connections: {e}")
        })?;

    task::spawn(async move {
        loop {
            let active_conns = async {
                let sample = sub.recv_async().await.map_err(|e| {
                    eyre!("failed to receive message from connd/oes/active_connections: {e}")
                })?;

                let active_conns: oes::ActiveConnections =
                    serde_json::from_slice(&sample.payload().to_bytes())
                        .context("failed to parse ActiveConnections json")?;

                Ok::<_, color_eyre::Report>(active_conns)
            };

            let active_conns = match active_conns.await {
                Ok(acs) => acs,
                Err(e) => {
                    error!("failed to update connection state: {e}");
                    continue;
                }
            };

            let has_internet = active_conns.connections.iter().any(|c| c.has_internet);
            is_online.store(has_internet, Ordering::Release);
        }
    });

    Ok(())
}
