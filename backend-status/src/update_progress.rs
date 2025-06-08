use orb_backend_status_dbus::types::{UpdateProgress, COMPLETED_PROGRESS};
use orb_update_agent_dbus::{ComponentState, ComponentStatus, UpdateAgentManagerProxy};
use std::ops::{Div, Mul};
use thiserror::Error;
use tracing::info;
use zbus::Connection;

pub struct UpdateProgressWatcher<'a> {
    connection: Connection,
    update_agent_manager_proxy: Option<UpdateAgentManagerProxy<'a>>,
}

#[derive(Debug, Error)]
pub enum UpdateProgressErr {
    #[error("failed to connect to dbus: {0}")]
    DbusConnect(zbus::Error),
    #[error("failed to perform RPC over dbus: {0}")]
    DbusRPC(zbus::Error),
}

impl UpdateProgressWatcher<'_> {
    pub async fn init(connection: &Connection) -> Result<Self, UpdateProgressErr> {
        Ok(Self {
            connection: connection.clone(),
            update_agent_manager_proxy: None,
        })
    }

    pub async fn poll_update_progress(
        &mut self,
    ) -> Result<UpdateProgress, UpdateProgressErr> {
        if self.update_agent_manager_proxy.is_none() {
            self.try_connect().await?;
        }

        match self
            .update_agent_manager_proxy
            .as_ref()
            .unwrap()
            .progress()
            .await
        {
            Ok(progress) => Ok(into_update_progress(&progress)),
            Err(e) => {
                info!("disconnected from update agent manager");
                self.update_agent_manager_proxy = None;
                Err(UpdateProgressErr::DbusRPC(e))
            }
        }
    }

    async fn try_connect(&mut self) -> Result<UpdateProgress, UpdateProgressErr> {
        let update_agent_manager_proxy =
            UpdateAgentManagerProxy::builder(&self.connection)
                .cache_properties(zbus::CacheProperties::No)
                .build()
                .await
                .map_err(UpdateProgressErr::DbusConnect)?;

        match update_agent_manager_proxy.progress().await {
            Ok(components) => {
                info!("connected to update agent manager");
                self.update_agent_manager_proxy = Some(update_agent_manager_proxy);
                Ok(into_update_progress(&components))
            }
            Err(e) => Err(UpdateProgressErr::DbusRPC(e)),
        }
    }
}

fn into_update_progress(components: &[ComponentStatus]) -> UpdateProgress {
    let total_progress = components.len() as u64 * 100;
    if total_progress == 0 {
        return UpdateProgress::completed();
    }
    let mut download_progress = components
        .iter()
        .filter(|c| {
            c.state == ComponentState::Downloading || c.state == ComponentState::Fetched
        })
        .map(|c| {
            if c.state == ComponentState::Downloading {
                c.progress as u64
            } else {
                COMPLETED_PROGRESS
            }
        })
        .sum::<u64>()
        .mul(100)
        .div(total_progress);
    let mut processed_progress = components
        .iter()
        .filter(|c| c.state == ComponentState::Processed)
        .map(|_| COMPLETED_PROGRESS) // consider completed once 'processed'
        .sum::<u64>()
        .mul(100)
        .div(total_progress);
    let install_progress = components
        .iter()
        .filter(|c| c.state == ComponentState::Installed)
        .map(|_| COMPLETED_PROGRESS) // consider completed once 'installed'
        .sum::<u64>()
        .mul(100)
        .div(total_progress);
    // if install starts, consider processed completed
    if install_progress > 0 {
        processed_progress = COMPLETED_PROGRESS;
    }
    // if processed starts, consider download completed
    if processed_progress > 0 {
        download_progress = COMPLETED_PROGRESS;
    }
    UpdateProgress {
        download_progress,
        processed_progress,
        install_progress,
        total_progress: (download_progress + install_progress + processed_progress) / 3,
        error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use eyre::{Result, WrapErr};
    use orb_update_agent_dbus::{UpdateAgentManagerT, UpdateAgentState};
    use std::sync::{Arc, Mutex};
    use zbus::ConnectionBuilder;

    type UpdateAgentManagerIface = orb_update_agent_dbus::UpdateAgentManager<Mocked>;

    #[derive(Clone, Debug)]
    struct Mocked {
        progress: Arc<Mutex<Vec<ComponentStatus>>>,
        overall_status: Arc<Mutex<UpdateAgentState>>,
    }

    // Note how we are simply implementing a trait from orb-attest-dbus instead of creating an entirely new struct with zbus macros.
    // This ensures that the function signatures all match up and we get good compile errors and LSP support.
    impl UpdateAgentManagerT for Mocked {
        fn progress(&self) -> Vec<ComponentStatus> {
            self.progress.lock().unwrap().clone()
        }

        fn overall_status(&self) -> UpdateAgentState {
            *self.overall_status.lock().unwrap()
        }

        fn overall_progress(&self) -> u8 {
            let components = self.progress.lock().unwrap();
            if components.is_empty() {
                return 100;
            }
            let total: u32 = components.iter().map(|c| c.progress as u32).sum();
            (total / components.len() as u32) as u8
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
            overall_status: Arc::new(Mutex::new(UpdateAgentState::Downloading)),
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
        let mut watcher = UpdateProgressWatcher::init(&client_connection)
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
            .poll_update_progress()
            .await
            .wrap_err("failed to poll update progress")?;

        // Verify the update was received correctly
        assert_eq!(progress.download_progress, 50);
        assert_eq!(progress.install_progress, 0);
        assert_eq!(progress.processed_progress, 0);
        assert_eq!(progress.total_progress, 16); // 50% of 1 of 3 steps completed

        Ok(())
    }

    #[test]
    fn test_into_update_progress() {
        // Test with a single downloading component
        let components = vec![ComponentStatus {
            name: "component1".to_string(),
            state: ComponentState::Downloading,
            progress: 50,
        }];

        let progress = into_update_progress(&components);
        assert_eq!(progress.download_progress, 50);
        assert_eq!(progress.install_progress, 0);
        assert_eq!(progress.processed_progress, 0);
        assert_eq!(progress.total_progress, 16); // 50% of 1 of 3 steps completed

        // Test with multiple components in different states
        let components = vec![
            ComponentStatus {
                name: "component1".to_string(),
                state: ComponentState::Downloading,
                progress: 60,
            },
            ComponentStatus {
                name: "component2".to_string(),
                state: ComponentState::Installed,
                progress: 40,
            },
            ComponentStatus {
                name: "component3".to_string(),
                state: ComponentState::Processed,
                progress: 20,
            },
        ];

        let progress = into_update_progress(&components);
        assert_eq!(progress.download_progress, 100);
        assert_eq!(progress.processed_progress, 100);
        assert_eq!(progress.install_progress, 33);
        assert_eq!(progress.total_progress, 77);

        // Test with empty components
        let components: Vec<ComponentStatus> = vec![];
        let progress = into_update_progress(&components);
        assert_eq!(progress.download_progress, 100);
        assert_eq!(progress.install_progress, 100);
        assert_eq!(progress.processed_progress, 100);
        assert_eq!(progress.total_progress, 100);
    }
}
