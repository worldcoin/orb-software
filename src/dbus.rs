//! Get current value of token
//! gdbus call --session -d org.worldcoin.AuthTokenManager -o '/org/worldcoin/AuthTokenManager' -m
//! org.freedesktop.DBus.Properties.Get org.worldcoin.AuthTokenManager Token
//!
//! Force token refresh
//! gdbus call --session -d org.worldcoin.AuthTokenManager -o '/org/worldcoin/AuthTokenManager' -m
//! org.worldcoin.AuthTokenManager.ForceTokenRefresh
//!
//! Wait for token refresh
//! dbus-monitor type='signal',sender='org.worldcoin.AuthTokenManager'

use std::sync::Arc;

use eyre::WrapErr;
use tokio::sync::Notify;
use tracing::warn;
use zbus::{dbus_interface, ConnectionBuilder};

pub struct AuthTokenManager {
    token: Option<String>,
    refresh_token_event: Arc<Notify>,
}

impl AuthTokenManager {
    #[must_use]
    pub fn new(refresh_token_event: Arc<Notify>) -> Self {
        AuthTokenManager {
            token: None,
            refresh_token_event,
        }
    }

    pub fn update_token(&mut self, token: &str) {
        self.token = Some(token.to_string());
    }
}

#[dbus_interface(name = "org.worldcoin.AuthTokenManager")]
impl AuthTokenManager {
    #[dbus_interface(property)]
    fn token(&self) -> (&str, &str) {
        match self.token.as_deref() {
            Some(token) if token.is_empty() => {
                warn!("token was Some(), but empty");
                ("", "backend returned empty token")
            }
            Some(token) => (token, ""),
            None => ("", "TODO some explanations"),
        }
    }

    #[allow(clippy::needless_pass_by_value)]
    #[allow(unused_variables)]
    fn force_token_refresh(&mut self, #[zbus(signal_context)] ctxt: zbus::SignalContext<'_>) {
        self.refresh_token_event.notify_one();
    }
}

/// Start the `AuthTokenManager` service
/// This service is used to provide the token to the rest of the system
/// It is also used to :
///  - force a token refresh
///  - to get the current token
///  - emit a signal when the token is changed
///
/// # Errors
/// - if failed to connect to the session bus or create the service
pub async fn create_dbus_connection(
    refresh_token_event: Arc<Notify>,
) -> eyre::Result<zbus::Connection> {
    let auth_token_manager = AuthTokenManager::new(refresh_token_event);
    let dbus = ConnectionBuilder::session()
        .wrap_err("failed to establish user session dbus connection")?
        .name("org.worldcoin.AuthTokenManager")
        .wrap_err(
            "failed to register the service under well-known name org.worldcoin.AuthTokenManager",
        )?
        .serve_at("/org/worldcoin/AuthTokenManager", auth_token_manager)
        .wrap_err("failed to serve at service path /org/worldcoin/AuthTokenManager")?
        .build()
        .await
        .wrap_err("failed to initialize the service on dbus")?;
    Ok(dbus)
}
