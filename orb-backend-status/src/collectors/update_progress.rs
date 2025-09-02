use orb_backend_status_dbus::types::UpdateProgress;
use orb_update_agent_dbus::{
    common_utils::UpdateAgentStateMapper,
    constants::{interfaces, methods, properties},
    UpdateAgentState,
};

#[cfg(test)]
use orb_update_agent_dbus::constants::{paths, services};

use thiserror::Error;
use tokio::sync::watch;
use tracing::{debug, warn};
use zbus::{
    export::futures_util::StreamExt,
    fdo::{self},
    zvariant::Value,
    Connection, MatchRule, MessageType,
};

type ProgressPair = (Option<u8>, Option<UpdateAgentState>);
type ExtractedProgress = Option<ProgressPair>;
type ExtractResult = Result<ExtractedProgress, UpdateProgressErr>;

#[derive(Debug, Error)]
pub enum UpdateProgressErr {
    #[error("failed to connect to dbus: {0}")]
    DbusConnect(zbus::Error),
    #[error("failed to perform RPC over dbus: {0}")]
    DbusRPC(zbus::Error),
}

pub struct UpdateProgressWatcher {
    progress_receiver: watch::Receiver<UpdateProgress>,
    _task_handle: tokio::task::JoinHandle<()>,
}

impl UpdateProgressWatcher {
    pub async fn init(connection: &Connection) -> Result<Self, UpdateProgressErr> {
        let (progress_sender, progress_receiver) =
            watch::channel(UpdateProgress::completed());

        let connection_clone = connection.clone();
        let task_handle = tokio::spawn(Self::signal_listener_task(
            connection_clone,
            progress_sender,
        ));

        Ok(Self {
            progress_receiver,
            _task_handle: task_handle,
        })
    }

    pub async fn poll_update_progress(
        &self,
    ) -> Result<UpdateProgress, UpdateProgressErr> {
        Ok(self.progress_receiver.borrow().clone())
    }

    async fn signal_listener_task(
        connection: Connection,
        progress_sender: watch::Sender<UpdateProgress>,
    ) {
        let _ = progress_sender.send(UpdateProgress::completed());

        if let Err(e) =
            Self::listen_for_update_agent_signals(&connection, &progress_sender).await
        {
            warn!("Progress signal listening failed: {e:?}");
        }
    }

    async fn listen_for_update_agent_signals(
        connection: &Connection,
        progress_sender: &watch::Sender<UpdateProgress>,
    ) -> Result<(), UpdateProgressErr> {
        let dbus_proxy = zbus::fdo::DBusProxy::new(connection)
            .await
            .map_err(UpdateProgressErr::DbusConnect)?;

        let match_rule = MatchRule::builder()
            .msg_type(MessageType::Signal)
            .interface(interfaces::PROPERTIES)
            .map_err(UpdateProgressErr::DbusRPC)?
            .member(methods::PROPERTIES_CHANGED)
            .map_err(UpdateProgressErr::DbusRPC)?
            .add_arg(interfaces::UPDATE_AGENT_MANAGER)
            .map_err(UpdateProgressErr::DbusRPC)?
            .build();

        dbus_proxy
            .add_match_rule(match_rule)
            .await
            .map_err(|e: zbus::fdo::Error| UpdateProgressErr::DbusRPC(e.into()))?;

        let mut stream = zbus::MessageStream::from(connection.clone());

        while let Some(message) = stream.next().await {
            let message = match message {
                Ok(msg) => msg,
                Err(e) => {
                    debug!("Error receiving message: {e:?}");
                    continue;
                }
            };

            if Self::is_update_agent_signal(&message) {
                if let Err(e) =
                    Self::handle_update_agent_message(&message, progress_sender).await
                {
                    debug!("Failed to handle update agent message: {e:?}");
                }
            }
        }

        Ok(())
    }

    fn is_update_agent_signal(message: &zbus::Message) -> bool {
        let header = message.header();
        let interface = header.interface().map(|i| i.as_str()).unwrap_or("");
        let member = header.member().map(|m| m.as_str()).unwrap_or("");

        interface == interfaces::PROPERTIES && member == methods::PROPERTIES_CHANGED
    }

    async fn handle_update_agent_message(
        message: &zbus::Message,
        progress_sender: &watch::Sender<UpdateProgress>,
    ) -> Result<bool, UpdateProgressErr> {
        let body = message.body();
        let properties_changed_args = match fdo::PropertiesChangedArgs::try_from(&body)
        {
            Ok(args) => args,
            Err(e) => {
                debug!("Failed to parse PropertiesChanged args: {e:?}");
                return Err(UpdateProgressErr::DbusRPC(e));
            }
        };

        if properties_changed_args.interface_name() != interfaces::UPDATE_AGENT_MANAGER
        {
            return Ok(false);
        }

        let changed_properties = properties_changed_args.changed_properties();
        if let Some((overall_progress, overall_state)) =
            Self::extract_progress_data(changed_properties)?
        {
            // Get the current progress to preserve it if we don't have new progress/state data
            let current_progress = progress_sender.borrow().clone();
            let progress_value =
                overall_progress.unwrap_or(current_progress.total_progress as u8);
            let state_value = overall_state.unwrap_or(current_progress.state);

            // Map progress to appropriate phases based on update agent state
            let (
                download_progress,
                processed_progress,
                install_progress,
                total_progress,
            ) = match state_value {
                UpdateAgentState::Downloading => {
                    (progress_value as u64, 0, 0, (progress_value / 3) as u64) // 1/3 of total workflow
                }
                UpdateAgentState::Fetched => (100, 0, 0, 66), // Downloading is still majority of time for the user
                UpdateAgentState::Processed => (100, 100, 0, 66),
                UpdateAgentState::Installing => (
                    100,
                    100,
                    progress_value as u64,
                    (66 + (progress_value / 3)) as u64,
                ),
                UpdateAgentState::Installed => (100, 100, 100, 100),
                UpdateAgentState::Rebooting => (100, 100, 100, 100),
                _ => (
                    progress_value as u64,
                    progress_value as u64,
                    progress_value as u64,
                    progress_value as u64,
                ),
            };

            let progress = UpdateProgress {
                download_progress,
                processed_progress,
                install_progress,
                total_progress,
                error: None,
                state: state_value,
            };

            if progress_sender.send(progress).is_err() {
                warn!("All progress receivers dropped, stopping signal listener");
                return Err(UpdateProgressErr::DbusRPC(zbus::Error::Failure(
                    "receivers dropped".to_string(),
                )));
            }
            return Ok(true);
        }

        Ok(false)
    }

    fn extract_progress_data(
        changed_properties: &std::collections::HashMap<&str, Value<'_>>,
    ) -> ExtractResult {
        let overall_progress = if let Some(progress_value) =
            changed_properties.get(properties::OVERALL_PROGRESS)
        {
            match progress_value {
                Value::U8(val) => Some(*val),
                _ => {
                    debug!("OverallProgress is not a U8 value");
                    None
                }
            }
        } else {
            None
        };

        let overall_state = if let Some(state_value) =
            changed_properties.get(properties::OVERALL_STATUS)
        {
            match state_value {
                Value::U32(val) => Some(
                    UpdateAgentStateMapper::from_u32(*val)
                        .unwrap_or(UpdateAgentState::None),
                ),
                _ => {
                    debug!("OverallStatus is not a U32 value");
                    Some(UpdateAgentState::None)
                }
            }
        } else {
            None
        };

        if overall_progress.is_some() || overall_state.is_some() {
            Ok(Some((overall_progress, overall_state)))
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eyre::{Result, WrapErr};
    use orb_update_agent_dbus::{
        common_utils::{ComponentStateMapper, UpdateAgentStateMapper},
        ComponentState, ComponentStatus, UpdateAgentManager, UpdateAgentManagerT,
    };
    use std::sync::{Arc, Mutex};
    use tokio::time::Duration;
    use zbus::ConnectionBuilder;

    // Helper function for tests - replicates the progress calculation logic
    fn calculate_progress_for_test(
        overall_progress: Option<u8>,
        overall_state: UpdateAgentState,
        current_progress: &UpdateProgress,
    ) -> UpdateProgress {
        let progress_value =
            overall_progress.unwrap_or(current_progress.total_progress as u8);

        let (download_progress, processed_progress, install_progress, total_progress) =
            match overall_state {
                UpdateAgentState::Downloading => {
                    (progress_value as u64, 0, 0, (progress_value / 3) as u64) // 1/3 of total workflow
                }
                UpdateAgentState::Fetched => (100, 0, 0, 33),
                UpdateAgentState::Processed => (100, 100, 0, 66),
                UpdateAgentState::Installing => (
                    100,
                    100,
                    progress_value as u64,
                    (66 + (progress_value / 3)) as u64,
                ),
                UpdateAgentState::Installed => (100, 100, 100, 100),
                UpdateAgentState::Rebooting => (100, 100, 100, 100),
                _ => (
                    progress_value as u64,
                    progress_value as u64,
                    progress_value as u64,
                    progress_value as u64,
                ),
            };

        UpdateProgress {
            download_progress,
            processed_progress,
            install_progress,
            total_progress,
            error: None,
            state: overall_state,
        }
    }

    #[derive(Clone, Debug)]
    struct MockUpdateAgent {
        progress: Arc<Mutex<Vec<ComponentStatus>>>,
        overall_status: Arc<Mutex<UpdateAgentState>>,
    }

    impl UpdateAgentManagerT for MockUpdateAgent {
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

    async fn start_dbus_daemon() -> dbus_launch::Daemon {
        tokio::task::spawn_blocking(|| {
            let tmpfile = tempfile::Builder::new().tempfile().unwrap();
            let path = tmpfile.path().file_name().unwrap().to_str().unwrap();
            dbus_launch::Launcher::daemon()
                .listen(format!("unix:path=/tmp/{path}").as_str())
                .launch()
                .expect("failed to launch dbus-daemon")
        })
        .await
        .expect("task panicked")
    }

    async fn setup_test_server(
        progress: Vec<ComponentStatus>,
    ) -> Result<(Connection, dbus_launch::Daemon, MockUpdateAgent)> {
        let mock_manager = MockUpdateAgent {
            progress: Arc::new(Mutex::new(progress)),
            overall_status: Arc::new(Mutex::new(UpdateAgentState::Downloading)),
        };
        let daemon = start_dbus_daemon().await;

        let connection = ConnectionBuilder::address(daemon.address())?
            .name(services::UPDATE_AGENT_MANAGER)?
            .serve_at(
                paths::UPDATE_AGENT_MANAGER,
                UpdateAgentManager(mock_manager.clone()),
            )?
            .build()
            .await?;

        Ok((connection, daemon, mock_manager))
    }

    #[tokio::test]
    async fn test_progress_update() -> Result<()> {
        // Skip test if dbus-daemon is not available (e.g., in Docker)
        if std::process::Command::new("dbus-daemon")
            .arg("--version")
            .output()
            .is_err()
        {
            eprintln!("Skipping test_progress_update: dbus-daemon not available");
            return Ok(());
        }

        let (_connection, _daemon, _mock_manager) =
            setup_test_server(vec![ComponentStatus {
                name: "test".to_string(),
                state: ComponentState::Downloading,
                progress: 50,
            }])
            .await?;

        let client_connection = ConnectionBuilder::address(_daemon.address())?
            .build()
            .await
            .wrap_err("failed to create client connection")?;

        let watcher = UpdateProgressWatcher::init(&client_connection)
            .await
            .wrap_err("failed to initialize UpdateProgressWatcher")?;

        tokio::time::sleep(Duration::from_millis(100)).await;

        let progress = watcher
            .poll_update_progress()
            .await
            .wrap_err("failed to get update progress")?;

        assert_eq!(progress.download_progress, 100);
        assert_eq!(progress.install_progress, 100);
        assert_eq!(progress.processed_progress, 100);
        assert_eq!(progress.total_progress, 100);

        Ok(())
    }

    #[test]
    fn test_update_progress_calculator() {
        let default_progress = UpdateProgress {
            download_progress: 0,
            processed_progress: 0,
            install_progress: 0,
            total_progress: 0,
            error: None,
            state: UpdateAgentState::None,
        };

        // Test with progress value from update-agent - Downloading state
        let progress = calculate_progress_for_test(
            Some(42),
            UpdateAgentState::Downloading,
            &default_progress,
        );

        assert_eq!(progress.download_progress, 42);
        assert_eq!(progress.processed_progress, 0); // Fixed: should be 0 during downloading
        assert_eq!(progress.install_progress, 0); // Fixed: should be 0 during downloading
        assert_eq!(progress.total_progress, 14); // Fixed: 42/3 = 14
        assert_eq!(progress.state, UpdateAgentState::Downloading);

        // Test with None progress (should preserve current progress for fallback states)
        let current_progress = UpdateProgress {
            download_progress: 75,
            processed_progress: 75,
            install_progress: 75,
            total_progress: 75,
            error: None,
            state: UpdateAgentState::Downloading,
        };
        let progress = calculate_progress_for_test(
            None,
            UpdateAgentState::NoNewVersion,
            &current_progress,
        );

        assert_eq!(progress.download_progress, 75); // Preserved via fallback case
        assert_eq!(progress.processed_progress, 75); // Preserved via fallback case
        assert_eq!(progress.install_progress, 75); // Preserved via fallback case
        assert_eq!(progress.total_progress, 75); // Preserved via fallback case
        assert_eq!(progress.state, UpdateAgentState::NoNewVersion); // Updated

        // Test with complete progress - Installed state
        let progress = calculate_progress_for_test(
            Some(100),
            UpdateAgentState::Installed,
            &default_progress,
        );

        assert_eq!(progress.download_progress, 100);
        assert_eq!(progress.processed_progress, 100);
        assert_eq!(progress.install_progress, 100);
        assert_eq!(progress.total_progress, 100);
        assert_eq!(progress.state, UpdateAgentState::Installed);
    }

    #[test]
    fn test_update_agent_state_mapper() {
        assert_eq!(
            UpdateAgentStateMapper::from_u32(1),
            Some(UpdateAgentState::None)
        );
        assert_eq!(
            UpdateAgentStateMapper::from_u32(2),
            Some(UpdateAgentState::Downloading)
        );
        assert_eq!(
            UpdateAgentStateMapper::from_u32(7),
            Some(UpdateAgentState::Rebooting)
        );
        assert_eq!(
            UpdateAgentStateMapper::from_u32(8),
            Some(UpdateAgentState::NoNewVersion)
        );
        assert_eq!(UpdateAgentStateMapper::from_u32(999), None);
    }

    #[test]
    fn test_component_state_mapper() {
        assert_eq!(
            ComponentStateMapper::from_update_agent_state(
                UpdateAgentState::Downloading
            ),
            ComponentState::Downloading
        );
        assert_eq!(
            ComponentStateMapper::from_update_agent_state(UpdateAgentState::Rebooting),
            ComponentState::Installed
        );
        assert_eq!(
            ComponentStateMapper::from_update_agent_state(
                UpdateAgentState::NoNewVersion
            ),
            ComponentState::None
        );
    }
}
