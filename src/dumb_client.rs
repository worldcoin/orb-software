//! A simple client, which gets a token from the daemon and a request to the backend, checking that
//! the token is still valid
//!
//! Usefull as a benchmark/test client

pub mod logging;

use eyre::{
    Result,
    WrapErr,
};
use futures::stream::StreamExt;
use tracing::{
    error,
    info,
    warn,
};
use zbus::{
    dbus_proxy,
    Connection,
};

const PING_URL: &str = {
    #[cfg(feature = "prod")]
    {
        "https://management.orb.worldcoin.org/api/v1/orbs"
    }
    #[cfg(not(feature = "prod"))]
    {
        "https://management.stage.orb.worldcoin.org/api/v1/orbs"
    }
};
const USER_AGENT: &str = "PingShortLivedToken/1.0";

#[dbus_proxy(
    interface = "org.worldcoin.AuthTokenManager1",
    default_service = "org.worldcoin.AuthTokenManager1",
    default_path = "/org/worldcoin/AuthTokenManager1"
)]
trait AuthToken {
    #[dbus_proxy(property)]
    fn token(&self) -> zbus::Result<String>;
}

/// Creates a new HTTP client.
fn client() -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .https_only(true)
        .build()
}

/// Contact the backend to verify that the token is still valid
///
/// Errors:
///
/// If failed to connect to the backend. If token is invalid the function is *successful*
async fn check_token(token: &str, orb_id: &str) -> Result<()> {
    let client = client()?;
    let resp = client
        .get(format!("{PING_URL}/{orb_id}"))
        .basic_auth(orb_id, Some(token))
        .send()
        .await?;
    if resp.status().is_success() {
        info!("Token is valid");
    } else {
        let msg = match resp.text().await {
            Ok(text) => text,
            Err(e) => {
                warn!(error=?e, "failed to read response body: {}", e);
                String::new()
            }
        };
        error!(text = msg, "Token is invalid");
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    logging::init();

    let orb_id = std::env::var("ORB_ID").wrap_err("env variable `ORB_ID` should be set")?;

    let connection = Connection::session().await?;
    let proxy = AuthTokenProxy::new(&connection).await?;

    let mut token_updated = proxy.receive_token_changed().await;

    if let Ok(token) = proxy.token().await {
        info!(token, "Got token");
        match check_token(&token, &orb_id).await {
            Ok(_) => {}
            Err(e) => error!(error=?e, "Failed to check token: {}", e),
        }
    } else {
        info!("Failed to get token at start");
    }

    while let Some(update) = token_updated.next().await {
        if let Ok(token) = update.get().await {
            info!(token = token, "Got token update");
            match check_token(&token, &orb_id).await {
                Ok(_) => {}
                Err(e) => error!(error=?e, "Failed to check token: {}", e),
            }
        } else {
            error!("Failed to get token update(dbus error)");
        }
    }
    Ok(())
}
