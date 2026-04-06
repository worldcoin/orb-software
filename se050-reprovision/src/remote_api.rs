//! This module is the lower level remote api.

use bon::Builder;
use color_eyre::eyre::{Result, WrapErr as _};
use orb_const_concat::const_concat;
use orb_endpoints::Backend;
use orb_info::OrbId;
use tracing::warn;

use self::client_builder::{IsUnset, SetBaseUrl, SetClient, State};
use crate::cli::KeyInfo;
use crate::BUILD_INFO;

const USER_AGENT: &str = const_concat!(
    "orb-se050-reprovision/",
    BUILD_INFO.cargo.pkg_version,
    "-",
    BUILD_INFO.git.describe,
);

#[derive(Debug, Clone, Builder)]
pub struct Client {
    // Make the default generated setter private, and rename
    // it so it doesn't collide with our custom method name
    #[builder(setters(vis = "", name = client_internal))]
    client: reqwest::Client,
    #[builder(setters(vis = "", name = base_url_internal))]
    base_url: String,
}

impl<S: State> ClientBuilder<S>
where
    S::Client: IsUnset,
{
    pub fn custom_reqwest_client(
        self,
        client: reqwest::Client,
    ) -> ClientBuilder<SetClient<S>> {
        self.client_internal(client)
    }

    pub fn default_reqwest_client(self) -> Result<ClientBuilder<SetClient<S>>> {
        let client = orb_security_utils::reqwest::http_client_builder()
            .user_agent(USER_AGENT)
            .build()
            .wrap_err("failed to create http client")?;

        Ok(self.client_internal(client))
    }
}

impl<S: State> ClientBuilder<S>
where
    S::BaseUrl: IsUnset,
{
    pub fn from_backend(self, backend: Backend) -> ClientBuilder<SetBaseUrl<S>> {
        let subdomain = match backend {
            Backend::Prod => "orb",
            Backend::Staging => "stage.orb",
            Backend::Analysis => "analysis.ml",
            Backend::Local => unreachable!(),
        };
        let base_url = format!("https://auth.{subdomain}.worldcoin.org");
        self.base_url_internal(base_url)
    }

    pub fn local_backend(self, port: u16) -> ClientBuilder<SetBaseUrl<S>> {
        self.base_url_internal(format!("http://localhost:{port}"))
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Challenge {
    orb_id: OrbId,
    /// Combined with orb_nonce for freshness
    server_nonce: u128,
}

#[derive(Debug, Clone, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PubkeyPem(String);

#[derive(Debug, Clone, Eq, PartialEq, serde::Serialize)]
pub struct Proof {
    #[serde(skip)]
    orb_id: OrbId,
    server_nonce: u128,
    /// Combined with server_nonce for freshness
    orb_nonce: u128,
    jetson_authkey: KeyInfo,
    attestation_key: KeyInfo,
    iris_code_key: KeyInfo,
}

impl Client {
    pub async fn start_challenge(
        &self,
        orb_id: &OrbId,
        legacy_bearer: Option<String>,
    ) -> Result<Challenge> {
        // TODO: Align with FM team on endpoints
        let url = format!("{}/api/v1/start_challenge/{}", self.base_url, orb_id);
        let request = self.client.put(&url);
        let request = if let Some(bearer) = legacy_bearer {
            request.bearer_auth(bearer)
        } else {
            warn!("no legacy bearer token provided, omitting it");
            request
        };
        let response = request
            .send()
            .await
            .wrap_err_with(|| format!("failed to transmit request to PUT {url}"))?
            .error_for_status()
            .wrap_err_with(|| format!("HTTP Error for PUT {url}"))?
            .bytes()
            .await
            .wrap_err_with(|| format!("failed to receive payload for PUT {url}"))?;

        let bytes = response.as_ref();
        let bytes: &[u8; 16] = bytes.try_into().wrap_err_with(|| {
            format!("bytes were wrong length, expected 16, got {}", bytes.len())
        })?;
        let server_nonce = u128::from_be_bytes(*bytes);

        Ok(Challenge {
            orb_id: orb_id.clone(),
            server_nonce,
        })
    }

    pub async fn finish_challenge(
        &self,
        proof: Proof,
        legacy_bearer: Option<String>,
    ) -> Result<()> {
        // TODO: Align with FM team on endpoints
        let url = format!("{}/api/v1/finish_challenge/{}", self.base_url, proof.orb_id);
        let request = self.client.put(&url);
        let request = if let Some(bearer) = legacy_bearer {
            request.bearer_auth(bearer)
        } else {
            warn!("no legacy bearer token provided, omitting it");
            request
        };
        request
            .json(&proof)
            .send()
            .await
            .wrap_err_with(|| format!("failed to transmit request to PUT {url}"))?
            .error_for_status()
            .wrap_err_with(|| format!("HTTP Error for PUT {url}"))?;

        Ok(())
    }
}
