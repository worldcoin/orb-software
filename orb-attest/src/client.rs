use futures::TryFutureExt;
use orb_const_concat::const_concat;
use reqwest::{Certificate, Client};
use secrecy::ExposeSecret;
use tokio::sync::OnceCell;
use tracing::{error, info, warn};

const USER_AGENT: &str = const_concat!(
    "ShortLivedTokenDaemon/",
    env!("CARGO_PKG_VERSION"),
    "-",
    env!("VERGEN_GIT_SHA"),
);

const AMAZON_ROOT_CA_1_PEM: &[u8] =
    include_bytes!("../ca-certificates/Amazon_Root_CA_1.pem");
const GTS_ROOT_R1_PEM: &[u8] = include_bytes!("../ca-certificates/GTS_Root_R1.pem");

static AMAZON_ROOT_CA_1_CERT: OnceCell<Certificate> = OnceCell::const_new();
static GTS_ROOT_R1_CERT: OnceCell<Certificate> = OnceCell::const_new();

static INSTANCE: OnceCell<Client> = OnceCell::const_new();

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("failed initializing HTTP client")]
    BuildClient(#[source] reqwest::Error),
    #[error("failed to connect to HTTP server")]
    ConnectionFailed(#[source] reqwest::Error),
    #[error("failed creating x509 certificate for AMAZON_ROOT_CA_1 from PEM bytes")]
    CreateAmazonRootCa1Cert(#[source] reqwest::Error),
    #[error("failed creating x509 certificate for GTS_ROOT_R1 from PEM bytes")]
    CreateGtsRootR1Cert(#[source] reqwest::Error),
}

/// Returns a shared instance of a [`Client`] with pinned Amazon Root CA 1 and GTS Root R1
/// certificates.
///
/// # Errors
/// - If initialization of the HTTP client failed
pub async fn get() -> Result<&'static Client, Error> {
    INSTANCE.get_or_try_init(initialize).await
}

async fn initialize() -> Result<Client, Error> {
    let amazon_cert = AMAZON_ROOT_CA_1_CERT
        .get_or_try_init(|| async { Certificate::from_pem(AMAZON_ROOT_CA_1_PEM) })
        .map_err(Error::CreateAmazonRootCa1Cert)
        .await?
        .clone();
    let google_cert = GTS_ROOT_R1_CERT
        .get_or_try_init(|| async { Certificate::from_pem(GTS_ROOT_R1_PEM) })
        .map_err(Error::CreateGtsRootR1Cert)
        .await?
        .clone();

    #[cfg(test)]
    let https_only = false;
    #[cfg(not(test))]
    let https_only = true;
    Client::builder()
        .add_root_certificate(amazon_cert)
        .add_root_certificate(google_cert)
        .min_tls_version(reqwest::tls::Version::TLS_1_3)
        .https_only(https_only)
        .redirect(reqwest::redirect::Policy::none())
        .tls_built_in_root_certs(false)
        .timeout(std::time::Duration::from_secs(60))
        .user_agent(USER_AGENT)
        .build()
        .map_err(Error::BuildClient)
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
    let client = get().await?;
    let resp = client
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
