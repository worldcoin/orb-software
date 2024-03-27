//! Get current value of token
//! gdbus call --session -d org.worldcoin.AuthTokenManager1 -o '/org/worldcoin/AuthTokenManager1' -m
//! org.freedesktop.DBus.Properties.Get org.worldcoin.AuthTokenManager1 Token
//!
//! Force token refresh
//! gdbus call --session -d org.worldcoin.AuthTokenManager1 -o '/org/worldcoin/AuthTokenManager1' -m
//! org.worldcoin.AuthTokenManager1.ForceTokenRefresh
//!
//! Wait for token refresh
//! dbus-monitor type='signal',sender='org.worldcoin.AuthTokenManager1'

use std::sync::Arc;

use eyre::WrapErr;
use tokio::sync::Notify;
use tracing::instrument;
use zbus::{interface, ConnectionBuilder};

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

#[interface(name = "org.worldcoin.AuthTokenManager1")]
impl AuthTokenManager {
    #[instrument(skip_all, err)]
    #[zbus(property)]
    fn token(&self) -> zbus::fdo::Result<&str> {
        match self.token.as_deref() {
            Some("") => Err(zbus::fdo::Error::Failed(
                "token was set, but is empty string".into(),
            )),
            Some(token) => Ok(token),
            None => Err(zbus::fdo::Error::Failed(
                "token was not yet or could not be retrieved from backend".into(),
            )),
        }
    }

    #[allow(clippy::needless_pass_by_value)]
    #[allow(unused_variables)]
    fn force_token_refresh(
        &mut self,
        #[zbus(signal_context)] ctxt: zbus::SignalContext<'_>,
    ) {
        self.refresh_token_event.notify_one();
    }
}

/// Start the `AuthTokenManager1` service
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
        .name("org.worldcoin.AuthTokenManager1")
        .wrap_err(
            "failed to register the service under well-known name org.worldcoin.AuthTokenManager",
        )?
        .serve_at("/org/worldcoin/AuthTokenManager1", auth_token_manager)
        .wrap_err("failed to serve at object path /org/worldcoin/AuthTokenManager1")?
        .build()
        .await
        .wrap_err("failed to initialize the service on dbus")?;
    Ok(dbus)
}
