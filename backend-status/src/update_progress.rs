use orb_backend_status_dbus::types::{UpdateProgress, COMPLETED_PROGRESS};
use orb_update_agent::common_utils::{ComponentStateMapper, UpdateAgentStateMapper};
use orb_update_agent_dbus::{ComponentState, ComponentStatus, UpdateAgentState};
use std::ops::{Div, Mul};
use thiserror::Error;
use tokio::sync::watch;
use tracing::{debug, warn};
use zbus::{
    export::futures_util::StreamExt,
    fdo::{self},
    zvariant::Value,
    Connection, MatchRule, MessageType,
};

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
        let (progress_sender, progress_receiver) = watch::channel(UpdateProgress::completed());
        
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

    pub async fn poll_update_progress(&mut self) -> Result<UpdateProgress, UpdateProgressErr> {
        Ok(self.progress_receiver.borrow().clone())
    }

    async fn signal_listener_task(
        connection: Connection,
        progress_sender: watch::Sender<UpdateProgress>,
    ) {
        let _ = progress_sender.send(UpdateProgress::completed());
        
        if let Err(e) = Self::listen_for_update_agent_signals(&connection, &progress_sender).await {
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
            .interface("org.freedesktop.DBus.Properties")
            .map_err(UpdateProgressErr::DbusRPC)?
            .member("PropertiesChanged")
            .map_err(UpdateProgressErr::DbusRPC)?
            .add_arg("org.worldcoin.UpdateAgentManager1")
            .map_err(UpdateProgressErr::DbusRPC)?
            .build();
        
        dbus_proxy.add_match_rule(match_rule).await
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
                if let Err(e) = Self::handle_update_agent_message(&message, progress_sender).await {
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
        
        interface == "org.freedesktop.DBus.Properties" && member == "PropertiesChanged"
    }

    async fn handle_update_agent_message(
        message: &zbus::Message,
        progress_sender: &watch::Sender<UpdateProgress>,
    ) -> Result<bool, UpdateProgressErr> {
        let body = message.body();
        tracing::info!("Received update agent message: {:?}", body);
        let properties_changed_args = match fdo::PropertiesChangedArgs::try_from(&body) {
            Ok(args) => args,
            Err(e) => {
                debug!("Failed to parse PropertiesChanged args: {e:?}");
                return Err(UpdateProgressErr::DbusRPC(e.into()));
            }
        };

        if properties_changed_args.interface_name() != "org.worldcoin.UpdateAgentManager1" {
            return Ok(false);
        }

        let changed_properties = properties_changed_args.changed_properties();
        tracing::info!("Changed properties: {:?}", changed_properties);
        if let Some((components, overall_state)) = Self::extract_progress_data(&changed_properties)? {
            let progress = UpdateProgressCalculator::from_components_with_state(&components, overall_state);
            
            if progress_sender.send(progress).is_err() {
                warn!("All progress receivers dropped, stopping signal listener");
                return Err(UpdateProgressErr::DbusRPC(zbus::Error::Failure("receivers dropped".to_string())));
            }
            return Ok(true);
        }

        Ok(false)
    }

    fn extract_progress_data(
        changed_properties: &std::collections::HashMap<&str, Value<'_>>
    ) -> Result<Option<(Vec<ComponentStatus>, UpdateAgentState)>, UpdateProgressErr> {
        let mut has_changes = false;
        
        let components = if let Some(progress_value) = changed_properties.get("Progress") {
            has_changes = true;
            match Self::parse_components_from_dbus_value(progress_value) {
                Ok(components) => components,
                Err(e) => {
                    debug!("Failed to parse Progress property: {}", e);
                    Vec::new() // Use empty vec on parse failure, don't bail out
                }
            }
        } else {
            Vec::new() // Default to empty vec if no Progress property
        };

        let overall_state = if let Some(state_value) = changed_properties.get("OverallStatus") {
            has_changes = true;
            match state_value {
                Value::U32(val) => {
                    UpdateAgentStateMapper::from_u32(*val).unwrap_or(UpdateAgentState::None)
                }
                _ => {
                    debug!("OverallStatus is not a U32 value");
                    UpdateAgentState::None
                }
            }
        } else {
            // Default to None if no overall status is provided
            UpdateAgentState::None
        };

        if has_changes {
            Ok(Some((components, overall_state)))
        } else {
            Ok(None) // Only return None if neither Progress nor OverallStatus changed
        }
    }

    fn parse_components_from_dbus_value(value: &Value<'_>) -> Result<Vec<ComponentStatus>, String> {
        match value {
            Value::Array(array) => {
                let mut components = Vec::new();
                
                for item in array.iter() {
                    if let Value::Structure(structure) = item {
                        let component = Self::parse_component_from_structure(structure)?;
                        components.push(component);
                    } else {
                        return Err("Array should contain structures".to_string());
                    }
                }
                
                Ok(components)
            }
            _ => Err("Value should be an array".to_string()),
        }
    }

    fn parse_component_from_structure(structure: &zbus::zvariant::Structure<'_>) -> Result<ComponentStatus, String> {
        let fields = structure.fields();
        if fields.len() != 3 {
            return Err(format!("Expected 3 fields in component struct, got {}", fields.len()));
        }
        
        let name = match &fields[0] {
            Value::Str(s) => s.as_str().to_string(),
            _ => return Err("First field should be string (name)".to_string()),
        };
        
        let update_agent_state = match &fields[1] {
            Value::U32(val) => {
                UpdateAgentStateMapper::from_u32(*val).unwrap_or(UpdateAgentState::None)
            }
            _ => return Err("Second field should be uint32 (state)".to_string()),
        };
        
        let state = ComponentStateMapper::from_update_agent_state(update_agent_state);
        
        let progress = match &fields[2] {
            Value::U8(val) => *val,
            _ => return Err("Third field should be byte (progress)".to_string()),
        };
        
        Ok(ComponentStatus {
            name,
            state,
            progress,
        })
    }
}

/// Calculates backend UpdateProgress from component statuses
struct UpdateProgressCalculator;

impl UpdateProgressCalculator {
    fn from_components_with_state(components: &[ComponentStatus], overall_state: UpdateAgentState) -> UpdateProgress {
        if components.is_empty() {
            return UpdateProgress {
                download_progress: 100,
                processed_progress: 100,
                install_progress: 100,
                total_progress: 100,
                error: None,
                state: overall_state, // ✅ Use the actual state, not None
            };
        }

        let total_components = components.len() as u64;
        let total_progress_points = total_components * 100;
        
        let download_progress = Self::calculate_phase_progress(
            components, 
            &[ComponentState::Downloading, ComponentState::Fetched],
            total_progress_points
        );
        let processed_progress = Self::calculate_phase_progress(
            components, 
            &[ComponentState::Processed],
            total_progress_points
        );
        let install_progress = Self::calculate_phase_progress(
            components, 
            &[ComponentState::Installed],
            total_progress_points
        );

        UpdateProgress {
            download_progress,
            processed_progress,
            install_progress,
            total_progress: (download_progress + processed_progress + install_progress) / 3,
            error: None,
            state: overall_state,
        }
    }

    fn calculate_phase_progress(
        components: &[ComponentStatus],
        target_states: &[ComponentState],
        total_progress_points: u64,
    ) -> u64 {
        let phase_progress: u64 = components
            .iter()
            .filter(|c| target_states.contains(&c.state))
            .map(|c| {
                if c.state == ComponentState::Downloading {
                    c.progress as u64
                } else {
                    COMPLETED_PROGRESS
                }
            })
            .sum();

        if total_progress_points == 0 {
            0
        } else {
            phase_progress.mul(100).div(total_progress_points)
        }
    }


}

#[cfg(test)]
mod tests {
    use super::*;
    use eyre::{Result, WrapErr};
    use orb_update_agent_dbus::{UpdateAgentManager};
    use std::sync::{Arc, Mutex};
    use tokio::time::Duration;
    use zbus::ConnectionBuilder;

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
            dbus_launch::Launcher::daemon()
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
            .name("org.worldcoin.UpdateAgentManager1")?
            .serve_at(
                "/org/worldcoin/UpdateAgentManager1",
                UpdateAgentManager(mock_manager.clone()),
            )?
            .build()
            .await?;

        Ok((connection, daemon, mock_manager))
    }

    #[tokio::test]
    async fn test_progress_update() -> Result<()> {
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

        let mut watcher = UpdateProgressWatcher::init(&client_connection)
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
        let components = vec![ComponentStatus {
            name: "component1".to_string(),
            state: ComponentState::Downloading,
            progress: 50,
        }];

        let progress = UpdateProgressCalculator::from_components_with_state(&components, UpdateAgentState::Downloading);
        assert_eq!(progress.download_progress, 50);
        assert_eq!(progress.install_progress, 0);
        assert_eq!(progress.processed_progress, 0);
        assert_eq!(progress.total_progress, 16);

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

        let progress = UpdateProgressCalculator::from_components_with_state(&components, UpdateAgentState::Installed);
        assert_eq!(progress.download_progress, 100);
        assert_eq!(progress.processed_progress, 100);
        assert_eq!(progress.install_progress, 33);
        assert_eq!(progress.total_progress, 77);

        let empty_components: Vec<ComponentStatus> = vec![];
        let progress = UpdateProgressCalculator::from_components_with_state(&empty_components, UpdateAgentState::None);
        assert_eq!(progress.download_progress, 100);
        assert_eq!(progress.install_progress, 100);
        assert_eq!(progress.processed_progress, 100);
        assert_eq!(progress.total_progress, 100);
    }

    #[test]
    fn test_update_agent_state_mapper() {
        assert_eq!(UpdateAgentStateMapper::from_u32(1), Some(UpdateAgentState::None));
        assert_eq!(UpdateAgentStateMapper::from_u32(2), Some(UpdateAgentState::Downloading));
        assert_eq!(UpdateAgentStateMapper::from_u32(7), Some(UpdateAgentState::NoNewVersion));
        assert_eq!(UpdateAgentStateMapper::from_u32(999), None);
    }

    #[test]
    fn test_component_state_mapper() {
        assert_eq!(ComponentStateMapper::from_update_agent_state(UpdateAgentState::Downloading), ComponentState::Downloading);
        assert_eq!(ComponentStateMapper::from_update_agent_state(UpdateAgentState::Rebooting), ComponentState::Installed);
        assert_eq!(ComponentStateMapper::from_update_agent_state(UpdateAgentState::NoNewVersion), ComponentState::None);
    }
}
