use std::time::Duration;

use once_cell::sync::OnceCell;
use reqwest::blocking::Client;

const APP_USER_AGENT: &str =
    concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"),);

static INSTANCE: OnceCell<Client> = OnceCell::new();

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("failed initializing HTTP client")]
    BuildClient(#[source] reqwest::Error),
}

// Return a HTTPS client with explicit reasonable defaults
pub fn normal() -> Result<&'static Client, Error> {
    INSTANCE.get_or_try_init(initialize)
}

fn initialize() -> Result<Client, Error> {
    // We explicitly do not pin certificates and default to using the system's
    // root CAs in the update-agent.
    //
    // This is to avoid a circumstance where an Orb falls out of sync with the
    // root CA's certificates and is unable to communicate with the update backend
    // after an extended period of going without updates.
    Client::builder()
        .tls_built_in_root_certs(true)
        .min_tls_version(reqwest::tls::Version::TLS_1_3)
        .redirect(reqwest::redirect::Policy::none())
        .https_only(true)
        .user_agent(APP_USER_AGENT)
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(Error::BuildClient)
}
