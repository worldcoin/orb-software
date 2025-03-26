use orb_update_agent_dbus::{ComponentState, ComponentStatus};
use thiserror::Error;
use zbus::export::futures_util::StreamExt;
use zbus::proxy::PropertyStream;
use zbus::{proxy, Connection};

use crate::dbus::UpdateProgress;

#[proxy(
    default_service = "org.worldcoin.UpdateAgentManager1",
    default_path = "/org/worldcoin/UpdateAgentManager1",
    interface = "org.worldcoin.UpdateAgentManager1"
)]
trait UpdateAgentManager {
    #[zbus(property)]
    fn progress(&self) -> zbus::Result<Vec<ComponentStatus>>;
}

pub struct UpdateProgressWatcher<'a> {
    _update_agent_manager_proxy: UpdateAgentManagerProxy<'a>,
    progress_update_stream: PropertyStream<'a, Vec<ComponentStatus>>,
}

#[derive(Debug, Error)]
pub enum UpdateProgressErr {
    #[error("failed to connect to dbus: {0}")]
    DbusConnect(zbus::Error),
    #[error("failed to perform RPC over dbus: {0}")]
    DbusRPC(zbus::Error),
}

impl UpdateProgressWatcher<'_> {
    pub async fn init(connection: Connection) -> Result<Self, UpdateProgressErr> {
        let update_agent_manager_proxy = UpdateAgentManagerProxy::new(&connection)
            .await
            .map_err(UpdateProgressErr::DbusConnect)?;
        let progress_update_stream =
            update_agent_manager_proxy.receive_progress_changed().await;

        Ok(Self {
            _update_agent_manager_proxy: update_agent_manager_proxy,
            progress_update_stream,
        })
    }

    pub async fn wait_for_updates(
        &mut self,
    ) -> Result<UpdateProgress, UpdateProgressErr> {
        if let Some(progress) = self.progress_update_stream.next().await {
            let progress = progress.get().await.map_err(UpdateProgressErr::DbusRPC)?;
            Ok(progress.into())
        } else {
            Err(UpdateProgressErr::DbusRPC(zbus::Error::Failure(
                "Disconnected".to_string(),
            )))
        }
    }
}

impl From<Vec<ComponentStatus>> for UpdateProgress {
    fn from(components: Vec<ComponentStatus>) -> Self {
        let total_progress = components.len() as u64 * 100;
        if total_progress == 0 {
            return UpdateProgress::default();
        }
        let download_progress = components
            .iter()
            .filter(|c| c.state == ComponentState::Downloading)
            .map(|c| c.progress as u64)
            .sum::<u64>();
        let install_progress = components
            .iter()
            .filter(|c| c.state == ComponentState::Installed)
            .map(|c| c.progress as u64)
            .sum::<u64>();
        let fetched_progress = components
            .iter()
            .filter(|c| c.state == ComponentState::Fetched)
            .map(|c| c.progress as u64)
            .sum::<u64>();
        let processed_progress = components
            .iter()
            .filter(|c| c.state == ComponentState::Processed)
            .map(|c| c.progress as u64)
            .sum::<u64>();
        UpdateProgress {
            download_progress: (download_progress * 100) / total_progress,
            install_progress: (install_progress * 100) / total_progress,
            fetched_progress: (fetched_progress * 100) / total_progress,
            processed_progress: (processed_progress * 100) / total_progress,
            total_progress: (download_progress
                + install_progress
                + fetched_progress
                + processed_progress)
                * 100
                / total_progress,
            errors: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use eyre::{Result, WrapErr};
    use orb_update_agent_dbus::UpdateAgentManagerT;
    use std::sync::{Arc, Mutex};
    use zbus::ConnectionBuilder;

    type UpdateAgentManagerIface = orb_update_agent_dbus::UpdateAgentManager<Mocked>;

    #[derive(Clone, Debug)]
    struct Mocked {
        progress: Arc<Mutex<Vec<ComponentStatus>>>,
    }

    // Note how we are simply implementing a trait from orb-attest-dbus instead of creating an entirely new struct with zbus macros.
    // This ensures that the function signatures all match up and we get good compile errors and LSP support.
    impl UpdateAgentManagerT for Mocked {
        fn progress(&self) -> Vec<ComponentStatus> {
            self.progress.lock().unwrap().clone()
        }
    }

    // using `dbus_launch` ensures that all tests use their own isolated dbus, and that they can't influence each other.
    async fn start_dbus_daemon() -> dbus_launch::Daemon {
        tokio::task::spawn_blocking(|| {
            dbus_launch::Launcher::daemon()
                .launch()
                .expect("failed to launch dbus-daemon")
        })
        .await
        .expect("task panicked")
    }

    async fn setup_test_server(
        progress: Vec<ComponentStatus>,
    ) -> Result<(Connection, dbus_launch::Daemon, Mocked)> {
        let mock_manager = Mocked {
            progress: Arc::new(Mutex::new(progress)),
        };
        let daemon = start_dbus_daemon().await;

        let connection = ConnectionBuilder::address(daemon.address())?
            .name("org.worldcoin.UpdateAgentManager1")?
            .serve_at(
                "/org/worldcoin/UpdateAgentManager1",
                orb_update_agent_dbus::UpdateAgentManager(mock_manager.clone()),
            )?
            .build()
            .await?;

        Ok((connection, daemon, mock_manager))
    }

    #[tokio::test]
    async fn test_progress_update() -> Result<()> {
        // Set up the test server
        let (connection, daemon, _mock_manager) =
            setup_test_server(vec![ComponentStatus {
                name: "test".to_string(),
                state: ComponentState::Downloading,
                progress: 50,
            }])
            .await?;
        let object_server = connection.object_server();
        let iface_ref = object_server
            .interface::<_, UpdateAgentManagerIface>(
                "/org/worldcoin/UpdateAgentManager1",
            )
            .await
            .wrap_err(
                "failed to get reference to UpdateAgentManager1 from object server",
            )?;

        // Create a client connection to the same bus
        let client_connection = ConnectionBuilder::address(daemon.address())?
            .build()
            .await
            .wrap_err("failed to create client connection")?;

        // Initialize the UpdateProgressWatcher
        let mut watcher = UpdateProgressWatcher::init(client_connection)
            .await
            .wrap_err("failed to initialize UpdateProgressWatcher")?;

        // Send the progress update signal
        iface_ref
            .get_mut()
            .await
            .progress_changed(iface_ref.signal_context())
            .await
            .wrap_err("failed to send progress_changed signal")?;

        // Wait for the update to be received
        let progress = watcher
            .wait_for_updates()
            .await
            .wrap_err("failed to wait for updates")?;

        // Verify the update was received correctly
        assert_eq!(progress.download_progress, 50);
        assert_eq!(progress.fetched_progress, 0);
        assert_eq!(progress.install_progress, 0);
        assert_eq!(progress.processed_progress, 0);
        assert_eq!(progress.total_progress, 50);
        assert_eq!(progress.errors, None);

        Ok(())
    }
}
