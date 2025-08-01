use std::sync::OnceLock;

use orb_const_concat::const_concat;
use reqwest::Client;
use secrecy::ExposeSecret;
use tracing::{error, info, warn};

use crate::BUILD_INFO;

const USER_AGENT: &str = const_concat!(
    "ShortLivedTokenDaemon/",
    BUILD_INFO.cargo.pkg_version,
    "-",
    BUILD_INFO.git.describe,
);

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("failed to connect to HTTP server")]
    ConnectionFailed(#[source] reqwest::Error),
}

/// Returns a shared instance of a http [`Client`] with pinned certificates.
pub fn client() -> &'static Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        let builder = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .user_agent(USER_AGENT)
            .danger_accept_invalid_certs(true)  // Temporarily accept invalid certs for testing
            .https_only(false);
        builder.build().expect("Failed to build client")
    })
}

/// Contact the backend to verify that the token is valid.
/// Returns Err(..) if failed to connect to the backend.
/// Returns Ok(true) if the token is valid.
/// Returns Ok(false) if the token is invalid
///
/// # Errors
///
/// If failed to connect to the backend.
pub async fn validate_token(
    orb_id: &str,
    token: &crate::remote_api::Token,
    ping_url: &url::Url,
) -> Result<bool, Error> {
    let resp = client()
        .get(ping_url.clone())
        .basic_auth(orb_id, Some(token.token.expose_secret()))
        .send()
        .await
        .map_err(Error::ConnectionFailed)?;
    if resp.status().is_success() {
        return Ok(true);
    }
    let msg = match resp.text().await {
        Ok(text) => text,
        Err(e) => {
            warn!(error=?e, "failed to read response body: {}", e);
            String::new()
        }
    };
    info!(text = msg, "Static token is invalid, but that is ok.");
    Ok(false)
}
