use crate::dbus::proxies::{
    SIGNUP_PROXY_DEFAULT_OBJECT_PATH, SIGNUP_PROXY_DEFAULT_WELL_KNOWN_NAME,
};
use orb_backend_status_dbus::types::SignupState;
use thiserror::Error;
use tokio::sync::watch;
use tracing::{info, warn};
use zbus::{
    export::futures_util::StreamExt, Connection, MatchRule, Message, MessageType,
};

#[derive(Debug, Error)]
pub enum CoreSignupError {
    #[error("failed to connect to dbus: {0}")]
    DbusConnect(zbus::Error),
    #[error("failed to perform RPC over dbus: {0}")]
    DbusRPC(zbus::Error),
}

pub struct CoreSignupWatcher {
    state_receiver: watch::Receiver<SignupState>,
    _task_handle: tokio::task::JoinHandle<()>,
}

impl CoreSignupWatcher {
    pub async fn init(connection: &Connection) -> Result<Self, CoreSignupError> {
        let (state_sender, state_receiver) = watch::channel(SignupState::default());

        let connection_clone = connection.clone();
        let task_handle =
            tokio::spawn(Self::signal_listener_task(connection_clone, state_sender));

        Ok(Self {
            state_receiver,
            _task_handle: task_handle,
        })
    }

    pub async fn get_signup_state(&self) -> Result<SignupState, CoreSignupError> {
        Ok(self.state_receiver.borrow().clone())
    }

    async fn signal_listener_task(
        connection: Connection,
        state_sender: watch::Sender<SignupState>,
    ) {
        if let Err(e) =
            Self::listen_for_signup_signals(&connection, &state_sender).await
        {
            warn!("Signup signal listening failed: {e:?}");
        }
    }

    async fn listen_for_signup_signals(
        connection: &Connection,
        state_sender: &watch::Sender<SignupState>,
    ) -> Result<(), CoreSignupError> {
        let dbus_proxy = zbus::fdo::DBusProxy::new(connection)
            .await
            .map_err(CoreSignupError::DbusConnect)?;

        // Match rule for signup signals
        let match_rule = MatchRule::builder()
            .msg_type(MessageType::Signal)
            .interface("org.worldcoin.OrbCore1.Signup")
            .map_err(CoreSignupError::DbusRPC)?
            .sender(SIGNUP_PROXY_DEFAULT_WELL_KNOWN_NAME)
            .map_err(CoreSignupError::DbusRPC)?
            .path(SIGNUP_PROXY_DEFAULT_OBJECT_PATH)
            .map_err(CoreSignupError::DbusRPC)?
            .build();

        dbus_proxy
            .add_match_rule(match_rule)
            .await
            .map_err(|e: zbus::fdo::Error| CoreSignupError::DbusRPC(e.into()))?;

        let mut stream = zbus::MessageStream::from(connection.clone());

        while let Some(message) = stream.next().await {
            let message = match message {
                Ok(msg) => msg,
                Err(e) => {
                    info!("Error receiving message: {e:?}");
                    continue;
                }
            };

            if Self::is_signup_signal(&message) {
                if let Err(e) = Self::handle_signup_signal(&message, state_sender).await
                {
                    info!("Failed to handle signup signal: {e:?}");
                }
            }
        }

        Ok(())
    }

    fn is_signup_signal(message: &Message) -> bool {
        let header = message.header();
        let interface = header.interface().map(|i| i.as_str()).unwrap_or("");
        let path = header.path().map(|p| p.as_str()).unwrap_or("");
        let sender = header.sender().map(|s| s.as_str()).unwrap_or("");

        interface == "org.worldcoin.OrbCore1.Signup"
            && path == SIGNUP_PROXY_DEFAULT_OBJECT_PATH
            && sender == SIGNUP_PROXY_DEFAULT_WELL_KNOWN_NAME
    }

    async fn handle_signup_signal(
        message: &Message,
        state_sender: &watch::Sender<SignupState>,
    ) -> Result<(), CoreSignupError> {
        let header = message.header();
        let member = header.member().map(|m| m.as_str()).unwrap_or("");

        info!("Received signup signal: {}", member);

        let new_state = match member {
            "signup_started" => {
                info!("Signup process started");
                SignupState::InProgress
            }
            "signup_finished" => {
                // Extract the success boolean from the message body
                let body = message.body();
                let success = body
                    .deserialize::<bool>()
                    .map_err(CoreSignupError::DbusRPC)?;

                info!("Signup process finished, success: {}", success);
                if success {
                    SignupState::CompletedSuccess
                } else {
                    SignupState::CompletedFailure
                }
            }
            "signup_ready" => {
                info!("System ready for signup");
                SignupState::Ready
            }
            "signup_not_ready" => {
                // Extract the reason string from the message body
                let body = message.body();
                let reason = body
                    .deserialize::<String>()
                    .map_err(CoreSignupError::DbusRPC)?;

                info!("System not ready for signup, reason: {}", reason);
                SignupState::NotReady
            }
            _ => {
                info!("Unknown signup signal: {}", member);
                return Ok(());
            }
        };

        if state_sender.send(new_state).is_err() {
            warn!("All signup state receivers dropped, stopping signal listener");
            return Err(CoreSignupError::DbusRPC(zbus::Error::Failure(
                "receivers dropped".to_string(),
            )));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eyre::Result;
    use std::time::Duration;
    use tokio::time::sleep;

    #[tokio::test]
    async fn test_signup_state_default() {
        let state = SignupState::default();
        assert_eq!(state, SignupState::Unknown);
    }

    #[test]
    fn test_signup_state_serialization() {
        let states = vec![
            SignupState::Ready,
            SignupState::NotReady,
            SignupState::InProgress,
            SignupState::CompletedSuccess,
            SignupState::CompletedFailure,
            SignupState::Unknown,
        ];

        for state in states {
            let serialized =
                serde_json::to_string(&state).expect("Failed to serialize");
            let deserialized: SignupState =
                serde_json::from_str(&serialized).expect("Failed to deserialize");
            assert_eq!(state, deserialized);
        }
    }

    #[tokio::test]
    async fn test_signup_watcher_initialization() -> Result<()> {
        // Skip test if dbus-daemon is not available (e.g., in Docker)
        if std::process::Command::new("dbus-daemon")
            .arg("--version")
            .output()
            .is_err()
        {
            eprintln!("Skipping test_signup_watcher_initialization: dbus-daemon not available");
            return Ok(());
        }

        let daemon = tokio::task::spawn_blocking(|| {
            dbus_launch::Launcher::daemon()
                .listen("tcp:host=localhost")
                .launch()
                .expect("failed to launch dbus-daemon")
        })
        .await
        .expect("task panicked");

        let connection = zbus::ConnectionBuilder::address(daemon.address())?
            .build()
            .await?;

        let watcher = CoreSignupWatcher::init(&connection).await?;

        // Initially should be Unknown state
        let initial_state = watcher.get_signup_state().await?;
        assert_eq!(initial_state, SignupState::Unknown);

        sleep(Duration::from_millis(100)).await;

        Ok(())
    }
}
