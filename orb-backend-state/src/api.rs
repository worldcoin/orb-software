//! HTTP requests to the orb-manager backend. This is purely network io.

use std::sync::OnceLock;

use color_eyre::{eyre::WrapErr, Result};
use derive_more::{Display, From};
use header_parsing::time_until_max_age;

use crate::state::State;

const API_BASE_URL: &str = "https://management.stage.orb.worldcoin.org";

/// Authorization token to the backend. Comes from short lived token daemon.
#[derive(Debug, Display, From, Clone)]
pub struct Token(String);

#[derive(Debug, Display, From, Clone)]
pub struct OrbId(String);

/// Retrieves the [`State`] from the backend.
pub async fn get_state(id: &OrbId, token: &Token) -> Result<State> {
    let c = get_http_client();
    let response = c
        .get(&format!("{API_BASE_URL}/api/v1/orbs/{id}/state"))
        .basic_auth(id, Some(token))
        .send()
        .await
        .wrap_err("Error while making get request for orb state to backend")?
        .error_for_status()
        .wrap_err("http status was an error")?;
    let expires_in = time_until_max_age(response.headers());
    let body = response
        .text()
        .await
        .wrap_err("Error while getting response body")?;
    Ok(State::new(body, expires_in))
}

fn get_http_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        orb_security_utils::reqwest::http_client_builder()
            .build()
            .expect("Failed to build client")
    })
}
