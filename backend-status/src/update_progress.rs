use orb_backend_status_dbus::types::{UpdateProgress, COMPLETED_PROGRESS};
use orb_update_agent_dbus::{ComponentState, ComponentStatus, UpdateAgentState};
use std::ops::{Div, Mul};
use thiserror::Error;
use tokio::sync::watch;
use tracing::{debug, info, warn};
use zbus::{
    export::futures_util::StreamExt,
    fdo::{self},
    zvariant::Value,
    Connection, MatchRule, MessageType,
};

pub struct UpdateProgressWatcher {
    progress_receiver: watch::Receiver<UpdateProgress>,
    _task_handle: tokio::task::JoinHandle<()>,
}

#[derive(Debug, Error)]
pub enum UpdateProgressErr {
    #[error("failed to connect to dbus: {0}")]
    DbusConnect(zbus::Error),
    #[error("failed to perform RPC over dbus: {0}")]
    DbusRPC(zbus::Error),
}

impl UpdateProgressWatcher {
    pub async fn init(connection: &Connection) -> Result<Self, UpdateProgressErr> {
        let (progress_sender, progress_receiver) = watch::channel(UpdateProgress::completed());
        
        info!("Initializing UpdateProgressWatcher with signal-based approach");
        
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
        // Return the latest progress from our signal-based cache
        let progress = self.progress_receiver.borrow().clone();
        debug!("Returning cached update progress: download={}, processed={}, install={}, total={}", 
               progress.download_progress, progress.processed_progress, 
               progress.install_progress, progress.total_progress);
        Ok(progress)
    }

    async fn signal_listener_task(
        connection: Connection,
        progress_sender: watch::Sender<UpdateProgress>,
    ) {
        info!("Starting update progress signal listener task");
        
        // Send completed progress initially
        let _ = progress_sender.send(UpdateProgress::completed());
        
        // Listen directly for progress change signals using add_match
        if let Err(e) = Self::listen_for_progress_signals(&connection, &progress_sender).await {
            warn!("Progress signal listening failed: {e:?}");
        }
    }

    async fn listen_for_progress_signals(
        connection: &Connection,
        progress_sender: &watch::Sender<UpdateProgress>,
    ) -> Result<(), UpdateProgressErr> {
        debug!("Starting direct signal listening for update progress");
        
        let dbus_proxy = zbus::fdo::DBusProxy::new(connection)
            .await
            .map_err(UpdateProgressErr::DbusConnect)?;

        let match_rule = MatchRule::builder()
            .msg_type(MessageType::Signal)
            .interface("org.freedesktop.DBus.Properties")
            .map_err(|e| UpdateProgressErr::DbusRPC(e))?
            .member("PropertiesChanged")
            .map_err(|e| UpdateProgressErr::DbusRPC(e))?
            .add_arg("org.worldcoin.UpdateAgentManager1")
            .map_err(|e| UpdateProgressErr::DbusRPC(e))?
            .build();
        
        debug!("Setting up direct signal matching for update progress changes");
        debug!("Match rule: {}", match_rule.to_string());
        
        dbus_proxy.add_match_rule(match_rule).await
            .map_err(|e: zbus::fdo::Error| UpdateProgressErr::DbusRPC(e.into()))?;
        
        debug!("Successfully added match rule, creating message stream");
        let mut stream = zbus::MessageStream::from(connection.clone());
        
        debug!("Now listening directly for progress change signals...");
        while let Some(message) = stream.next().await {
            let message = match message {
                Ok(msg) => {
                    debug!("Received D-Bus message");
                    msg
                },
                Err(e) => {
                    debug!("Error receiving message: {e:?}");
                    continue;
                }
            };

            // Check if this is the signal we're interested in
            let header = message.header();
            let sender = header.sender().map(|s| s.as_str()).unwrap_or("");
            let interface = header.interface().map(|i| i.as_str()).unwrap_or("");
            let member = header.member().map(|m| m.as_str()).unwrap_or("");
            
            debug!("Message details - sender: '{}', interface: '{}', member: '{}'", sender, interface, member);
            
            if interface == "org.freedesktop.DBus.Properties"
                && member == "PropertiesChanged"
            {
                debug!("Received progress change signal - matches our criteria!");
                
                // Handle PropertiesChanged message directly
                if let Err(e) = Self::handle_properties_changed_message(&message, connection, progress_sender).await {
                    debug!("Failed to handle properties changed message: {e:?}");
                    // Continue listening for more signals
                }
            }
        }

        Ok(())
    }

    async fn handle_properties_changed_message(
        message: &zbus::Message,
        _connection: &Connection,
        progress_sender: &watch::Sender<UpdateProgress>,
    ) -> Result<bool, UpdateProgressErr> {
        // Get the message body first to avoid lifetime issues
        let body = message.body();
        
        // Try to extract PropertiesChanged arguments from the message body
        let properties_changed_args = match fdo::PropertiesChangedArgs::try_from(&body) {
            Ok(args) => args,
            Err(e) => {
                debug!("Failed to parse PropertiesChanged args: {e:?}");
                return Err(UpdateProgressErr::DbusRPC(e.into()));
            }
        };

        // Check if this is for our interface
        if properties_changed_args.interface_name() != "org.worldcoin.UpdateAgentManager1" {
            debug!("PropertiesChanged signal not for our interface: {}", properties_changed_args.interface_name());
            return Ok(false);
        }

        debug!("PropertiesChanged signal for UpdateAgentManager1 interface");
        let changed_properties = properties_changed_args.changed_properties();
        
        // Log what properties changed
        for (prop_name, _value) in changed_properties.iter() {
            debug!("Property '{}' changed", prop_name);
        }

        // Check if any of our properties of interest changed
        let has_progress = changed_properties.contains_key("Progress");
        let has_overall_status = changed_properties.contains_key("OverallStatus");
        let has_overall_progress = changed_properties.contains_key("OverallProgress");

        if has_progress || has_overall_status || has_overall_progress {
            info!("Update-related properties changed: Progress={}, OverallStatus={}, OverallProgress={}", 
                  has_progress, has_overall_status, has_overall_progress);
            
            // Parse values directly from the signal
            let mut components: Option<Vec<ComponentStatus>> = None;
            let mut overall_status: Option<UpdateAgentState> = None;
            let mut overall_progress: Option<u8> = None;
            
            for (prop_name, value) in changed_properties.iter() {
                debug!("Processing property: {} = {:?}", prop_name, value);
                match prop_name.as_ref() {
                    "Progress" => {
                        // Parse Vec<ComponentStatus> from the D-Bus array of structs
                        match parse_progress_from_value(value) {
                            Ok(comp_vec) => {
                                components = Some(comp_vec);
                                debug!("Extracted Progress from signal: {} components", components.as_ref().unwrap().len());
                            }
                            Err(e) => {
                                debug!("Failed to parse Progress property from signal: {}", e);
                            }
                        }
                    }
                    "OverallStatus" => {
                        // Parse UpdateAgentState from the value (it's a uint32)
                        if let Ok(status_val) = <u32>::try_from(value) {
                            // Convert u32 to UpdateAgentState
                            overall_status = match status_val {
                                1 => Some(UpdateAgentState::None),
                                2 => Some(UpdateAgentState::Downloading),
                                3 => Some(UpdateAgentState::Fetched),
                                4 => Some(UpdateAgentState::Processed),
                                5 => Some(UpdateAgentState::Installed),
                                6 => Some(UpdateAgentState::Rebooting),
                                7 => Some(UpdateAgentState::NoNewVersion),
                                _ => Some(UpdateAgentState::None),
                            };
                            debug!("Extracted OverallStatus from signal: {:?} ({})", overall_status, status_val);
                        } else {
                            debug!("Failed to parse OverallStatus property from signal");
                        }
                    }
                    "OverallProgress" => {
                        // Parse u8 from the value
                        if let Ok(progress_val) = <u8>::try_from(value) {
                            overall_progress = Some(progress_val);
                            debug!("Extracted OverallProgress from signal: {}%", progress_val);
                        } else {
                            debug!("Failed to parse OverallProgress property from signal");
                        }
                    }
                    _ => {}
                }
            }
            
            // Process the extracted values
            if let Some(components) = components {
                let progress = into_update_progress(&components);
                info!("Update progress from signal: download={}, processed={}, install={}, total={}", 
                      progress.download_progress, progress.processed_progress, 
                      progress.install_progress, progress.total_progress);
                
                if let Some(status) = overall_status {
                    info!("Overall update status from signal: {:?}", status);
                }
                
                if let Some(progress_pct) = overall_progress {
                    info!("Overall progress from signal: {}%", progress_pct);
                }
                
                // Log component details
                for component in &components {
                    debug!("Component '{}': state={:?}, progress={}%", 
                           component.name, component.state, component.progress);
                }
                
                if progress_sender.send(progress).is_err() {
                    warn!("All progress receivers dropped, stopping signal listener");
                    return Err(UpdateProgressErr::DbusRPC(zbus::Error::Failure("receivers dropped".to_string())));
                }
            } else {
                // If we don't have Progress but have other properties, we still want to log them
                if let Some(status) = overall_status {
                    info!("Overall update status from signal: {:?}", status);
                }
                
                if let Some(progress_pct) = overall_progress {
                    info!("Overall progress from signal: {}%", progress_pct);
                }
                
                debug!("No Progress component data in signal, keeping current progress state");
            }
            
            return Ok(true);
        } else {
            debug!("PropertiesChanged signal for UpdateAgentManager1 but no relevant properties changed");
            return Ok(false);
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

/// Parse ComponentStatus array from D-Bus Value
/// The value should be an array of structs: (string, uint32, byte)
/// representing (name, state, progress)
fn parse_progress_from_value(value: &Value<'_>) -> Result<Vec<ComponentStatus>, String> {
    match value {
        Value::Array(array) => {
            let mut components = Vec::new();
            
            for item in array.iter() {
                match item {
                    Value::Structure(structure) => {
                        let fields = structure.fields();
                        if fields.len() != 3 {
                            return Err(format!("Expected 3 fields in component struct, got {}", fields.len()));
                        }
                        
                        // Extract name (string)
                        let name = match &fields[0] {
                            Value::Str(s) => s.as_str().to_string(),
                            _ => return Err("First field should be string (name)".to_string()),
                        };
                        
                        // Extract state (uint32)
                        let state_val = match &fields[1] {
                            Value::U32(val) => *val,
                            _ => return Err("Second field should be uint32 (state)".to_string()),
                        };
                        
                        // Convert state number to ComponentState enum
                        let state = match state_val {
                            1 => ComponentState::None,
                            2 => ComponentState::Downloading,
                            3 => ComponentState::Fetched,
                            4 => ComponentState::Processed,
                            5 => ComponentState::Installed,
                            _ => ComponentState::None, // Default fallback
                        };
                        
                        // Extract progress (byte)
                        let progress = match &fields[2] {
                            Value::U8(val) => *val,
                            _ => return Err("Third field should be byte (progress)".to_string()),
                        };
                        
                        components.push(ComponentStatus {
                            name,
                            state,
                            progress,
                        });
                    }
                    _ => return Err("Array should contain structures".to_string()),
                }
            }
            
            Ok(components)
        }
        _ => Err("Value should be an array".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use eyre::{Result, WrapErr};
    use orb_update_agent_dbus::{UpdateAgentManagerT, UpdateAgentState};
    use std::sync::{Arc, Mutex};
    use tokio::time::Duration;
    use zbus::ConnectionBuilder;

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
        let (_connection, _daemon, _mock_manager) =
            setup_test_server(vec![ComponentStatus {
                name: "test".to_string(),
                state: ComponentState::Downloading,
                progress: 50,
            }])
            .await?;

        // Create a client connection to the same bus
        let client_connection = ConnectionBuilder::address(_daemon.address())?
            .build()
            .await
            .wrap_err("failed to create client connection")?;

        // Initialize the UpdateProgressWatcher
        let mut watcher = UpdateProgressWatcher::init(&client_connection)
            .await
            .wrap_err("failed to initialize UpdateProgressWatcher")?;

        // Wait a moment for the initial setup
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Should return completed progress initially (signal-based approach starts with completed)
        let progress = watcher
            .poll_update_progress()
            .await
            .wrap_err("failed to get update progress")?;
        
        // With the new signal-based approach, we start with completed progress
        // and only update when signals are received (which is the correct behavior)
        assert_eq!(progress.download_progress, 100);
        assert_eq!(progress.install_progress, 100);
        assert_eq!(progress.processed_progress, 100);
        assert_eq!(progress.total_progress, 100);

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
