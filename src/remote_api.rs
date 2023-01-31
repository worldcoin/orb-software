use std::{
    io::Write,
    process::{
        Command,
        Stdio,
    },
};

use data_encoding::BASE64;
use ring::{
    digest,
    digest::digest,
};
use serde::{
    Deserialize,
    Serialize,
};
use serde_with::{
    base64::Base64,
    serde_as,
};
use tokio::{
    sync::OnceCell,
    time::{
        self,
        sleep,
    },
};
use tracing::{
    error,
    event,
    info,
    warn,
    Level,
};
use url::Url;

#[cfg(feature = "prod")]
const BASE_AUTH_URL: &str = "https://auth.orb.worldcoin.dev/api/v1/";
#[cfg(not(feature = "prod"))]
const BASE_AUTH_URL: &str = "https://auth.stage.orb.worldcoin.dev/api/v1/";

static GET_CHALLENGE_URL: OnceCell<Url> = OnceCell::const_new();
static GET_AUTH_TOKEN_URL: OnceCell<Url> = OnceCell::const_new();

async fn get_challenge_url() -> &'static Url {
    GET_CHALLENGE_URL
        .get_or_init(|| async {
            Url::parse(&(BASE_AUTH_URL.to_string() + "tokenchallenge"))
                .expect("CHALLENGE_URL should be a valid URL")
        })
        .await
}

async fn get_auth_token_url() -> &'static Url {
    GET_AUTH_TOKEN_URL
        .get_or_init(|| async {
            Url::parse(&(BASE_AUTH_URL.to_string() + "token"))
                .expect("AUTH_TOKEN_URL should be a valid URL")
        })
        .await
}

/// Number of attempts to fetch the challenge from the backend before giving up
const NUMBER_OF_CHALLENGE_RETRIES: u32 = 3;
/// How long to wait before retrying to fetch the challenge
const CHALLENGE_DELAY: time::Duration = time::Duration::from_secs(5);
/// Number of attempts to sign the challenge
const NUMBER_OF_SIGNINIG_RETRIES: u32 = 3;
/// How long to wait before retrying signing
const SIGNING_DELAY: time::Duration = time::Duration::from_secs(5);
/// Number of attempts to fetch the token
const NUMBER_OF_TOKEN_FETCH_RETRIES: u32 = NUMBER_OF_CHALLENGE_RETRIES;
/// How long to wait before retrying to fetch the token
const TOKEN_DELAY: time::Duration = CHALLENGE_DELAY;

#[derive(Debug, thiserror::Error)]
pub enum ChallengeError {
    #[error("failed to initialized HTTP client")]
    HTTPClientInitFailed(#[source] crate::client::Error),
    #[error("HTTP challenge request failed")]
    PostFailed(#[source] reqwest::Error),
    #[error("failed to parse JSON response")]
    JsonParseFailed(#[source] reqwest::Error),
    #[error("Server returned error")]
    ServerReturnedError(reqwest::StatusCode, String),
}

#[derive(Debug, thiserror::Error)]
pub enum SignError {
    #[error("no sign binary is found on system")]
    NoSignBinary,
    #[error("failed to spawn sign tool")]
    SpawnFailed(#[source] std::io::Error),
    #[error("failed to write to sign tool stdin")]
    WriteFailed(#[source] std::io::Error),
    #[error("failed to read from to sign tool stdout")]
    ReadFailed(#[source] std::io::Error),
    #[error("sign tool failed to sign the challenge")]
    SignFailed,
    #[error("se is not provisioned")]
    NotProvisioned,
    #[error("sign tool does not like input")]
    BadInput,
    #[error("sign tool experienced an internal error")]
    InternalError,
    #[error("non-zero exit code: {0}")]
    NonZeroExitCode(i32),
    #[error("terminated by signal")]
    TerminatedBySignal,
    #[error("signing on SE timed out")]
    Timeout,
    #[error("incomprehensible output")]
    BadOutput(#[source] data_encoding::DecodeError),
}

#[derive(Debug, thiserror::Error)]
pub enum TokenError {
    #[error("failed to initialized HTTP client")]
    HTTPClientInitFailed(#[source] crate::client::Error),
    #[error("post request to the server failed")]
    PostFailed(#[source] reqwest::Error),
    #[error("server returned error status code {0} with body \"{1}\"")]
    ServerReturnedError(reqwest::StatusCode, String),
    #[error("failed to parse JSON response")]
    JsonParseFailed(#[source] reqwest::Error),
    #[error("token field is empty in the response")]
    EmptyResponse,
}

#[derive(Debug, thiserror::Error)]
pub enum RefreshTokenError {
    #[error("failed to fetch challenge")]
    ChallengeError(#[source] ChallengeError),
    #[error("failed to sign challenge")]
    SignError(#[source] SignError),
    #[error("failed to fetch token")]
    TokenError(#[source] TokenError),
    #[error("challenge token expired before we could fetch a token")]
    ChallengeExpired,
    #[error("encountered panic while singing the challenge")]
    JoinError(#[source] tokio::task::JoinError),
}

#[serde_as]
#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct Challenge {
    #[serde(rename = "challenge")]
    #[serde_as(as = "Base64")]
    challenge: Vec<u8>,
    /// Challenge validity period in seconds
    #[serde_as(as = "serde_with::DurationSeconds<u64>")]
    duration: std::time::Duration,
    /// Challenge expiration time in server time. It is not used by the client
    /// and deliberately left unparsed as 'String' to avoid any parsing failures
    /// if the format changes.
    #[serde(rename = "expiryTime")]
    expiry_time: String,
    #[serde(skip, default = "time::Instant::now")]
    start_time: time::Instant,
}

impl Challenge {
    #[tracing::instrument]
    pub async fn request(orb_id: &str, url: &url::Url) -> Result<Self, ChallengeError> {
        let client = crate::client::get()
            .await
            .map_err(ChallengeError::HTTPClientInitFailed)?;

        let req = serde_json::json!({
            "orbId": orb_id,
        });

        info!("requesting challenge from {}", url);

        let resp = client
            .post(url.clone())
            .json(&req)
            .send()
            .await
            .map_err(ChallengeError::PostFailed)?;

        let status = resp.status();
        if status.is_client_error() || status.is_server_error() {
            let msg = match resp.text().await {
                Ok(text) => text,
                Err(e) => {
                    warn!("failed to read response body: {}", e);
                    String::new()
                }
            };
            // TODO change from satus to error
            Err(ChallengeError::ServerReturnedError(status, msg))
        } else {
            match resp.json::<Challenge>().await {
                Ok(challenge) => Ok(challenge),
                Err(e) => {
                    // TODO dump text of the response to see what is actually returned
                    error!("failed to parse challenge response: {}", e);
                    Err(ChallengeError::JsonParseFailed(e))
                }
            }
        }
    }

    /// Return true if the challenge is expired, The challenge is consider as
    /// expired if the remaining time is less that *half* of expiry time
    /// reported by the auth server. This guarantees that the challenge is always fresh.
    #[must_use]
    pub fn expired(&self) -> bool {
        let elapsed = self.start_time.elapsed();
        elapsed > (self.duration / 2)
    }

    /// Try to sign the challenge using SE050. Could fail for multiple reasons,
    /// it is probably a good idea to retry signing.
    #[tracing::instrument]
    pub fn sign(&self) -> Result<Signature, SignError> {
        let digest = digest(&digest::SHA256, &self.challenge);

        let encoded = BASE64.encode(digest.as_ref());

        let command = Command::new("orb-sign-attestation")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(SignError::SpawnFailed)?;

        writeln!(
            command.stdin.as_ref().expect("child should have stdin"),
            "{encoded}"
        )
        .map_err(SignError::WriteFailed)?;

        // TODO add timeout of 10-20 seconds (or use tokio::process::CommandExt::timeout)
        let output = command.wait_with_output().map_err(SignError::ReadFailed)?;
        let sign_tool_log = String::from_utf8_lossy(&output.stderr);
        event!(Level::INFO, sign_tool_log = ?sign_tool_log, orb_sign_attestation_success = output.status.success(), orb_sign_attestation_code = output.status.code());

        // TODO check errkind
        if !output.status.success() {
            return match output.status.code() {
                Some(127) => Err(SignError::NoSignBinary),
                Some(1) => Err(SignError::SignFailed),
                Some(2) => Err(SignError::Timeout),
                Some(3) => Err(SignError::NotProvisioned),
                Some(4) => Err(SignError::BadInput),
                Some(5) => Err(SignError::InternalError),
                Some(code) => Err(SignError::NonZeroExitCode(code)),
                None => Err(SignError::TerminatedBySignal),
            };
        }
        Ok(Signature {
            signature: BASE64
                .decode(&output.stdout)
                .map_err(SignError::BadOutput)?,
        })
    }
}

#[serde_as]
#[derive(Debug, Serialize)]
pub struct Signature {
    #[serde(rename = "Signature")]
    #[serde_as(as = "Base64")]
    signature: Vec<u8>,
}

#[serde_as]
#[derive(Serialize)]
struct TokenRequest {
    #[serde(rename = "orbId")]
    orb_id: String,
    #[serde(rename = "challenge")]
    #[serde_as(as = "Base64")]
    challenge: Vec<u8>,
    #[serde(rename = "signature")]
    #[serde_as(as = "Base64")]
    signature: Vec<u8>,
}

#[serde_as]
#[derive(Deserialize)]
#[allow(dead_code)]
pub struct Token {
    /// token value
    #[serde(rename = "token")]
    pub token: String,
    /// token validity period in seconds
    #[serde_as(as = "serde_with::DurationSeconds<u64>")]
    pub duration: std::time::Duration,
    /// token expiration time in server time
    #[serde(rename = "expiryTime")]
    expiry_time: String,
    /// local time when the token was fetched
    #[serde(skip, default = "time::Instant::now")]
    pub start_time: time::Instant,
}

impl Token {
    #[tracing::instrument]
    pub async fn request(
        url: &url::Url,
        orb_id: &str,
        challenge: &Challenge,
        signature: &Signature,
    ) -> Result<Self, TokenError> {
        let client = crate::client::get()
            .await
            .map_err(TokenError::HTTPClientInitFailed)?;

        info!("requesting token from {}", url);

        let req = TokenRequest {
            orb_id: orb_id.to_string(),
            challenge: challenge.challenge.clone(),
            signature: signature.signature.clone(),
        };

        let resp = client
            .post(url.clone())
            .json(&req)
            .send()
            .await
            .map_err(TokenError::PostFailed)?;

        let status = resp.status();
        if status.is_client_error() || status.is_server_error() {
            let msg = match resp.text().await {
                Ok(text) => text,
                Err(e) => {
                    warn!("failed to read response body: {}", e);
                    String::new()
                }
            };
            Err(TokenError::ServerReturnedError(status, msg))
        } else {
            match resp.json::<Token>().await {
                Ok(token) if token.token.is_empty() => Err(TokenError::EmptyResponse),
                Ok(token) => Ok(token),
                Err(e) => {
                    error!("failed to parse token response: {}", e);
                    Err(TokenError::JsonParseFailed(e))
                }
            }
        }
    }

    /// Return true if the token is expired.
    ///
    /// The token is considered expired if the remaining time is less that 1
    /// hour before the expiration time reported by the auth server.
    #[must_use]
    pub fn is_expired(&self) -> bool {
        let elapsed = self.start_time.elapsed();
        elapsed > (self.duration - std::time::Duration::from_secs(3600))
    }

    #[must_use]
    pub fn get_best_refresh_time(&self) -> std::time::Duration {
        let elapsed = self.start_time.elapsed();
        self.duration / 2 - elapsed
    }
}

impl std::string::ToString for Token {
    fn to_string(&self) -> String {
        self.token.clone()
    }
}

/// Try to refresh the token once, if it succeeds, return the new token.
#[tracing::instrument]
async fn get_token_inner(
    orb_id: &str,
    token_challenge: &url::Url,
    token_fetch: &url::Url,
) -> Result<Token, RefreshTokenError> {
    let mut retry = 0;

    let challenge = loop {
        retry += 1;
        let val = Challenge::request(orb_id, token_challenge)
            .await
            .map_err(RefreshTokenError::ChallengeError);
        if retry >= NUMBER_OF_CHALLENGE_RETRIES {
            break val?;
        }
        match val {
            Ok(challenge) => break challenge,
            Err(e) => {
                error!("failed to get challenge: {}", e);
                sleep(CHALLENGE_DELAY).await;
            }
        }
    };

    retry = 0;
    let signature = loop {
        retry += 1;
        if challenge.expired() {
            return Err(RefreshTokenError::ChallengeExpired);
        }

        let clone_of_challenge = challenge.clone();
        let val = tokio::task::spawn_blocking(move || {
            clone_of_challenge
                .sign()
                .map_err(RefreshTokenError::SignError)
        })
        .await
        .map_err(RefreshTokenError::JoinError)?;

        if retry >= NUMBER_OF_SIGNINIG_RETRIES {
            break val?;
        }
        match val {
            Ok(signature) => break signature,
            Err(e) => {
                error!("failed to sign challenge: {}", e);
                sleep(SIGNING_DELAY).await;
            }
        }
    };

    retry = 0;
    let token = loop {
        retry += 1;
        if challenge.expired() {
            return Err(RefreshTokenError::ChallengeExpired);
        }
        let val = Token::request(token_fetch, orb_id, &challenge, &signature)
            .await
            .map_err(RefreshTokenError::TokenError);
        if retry >= NUMBER_OF_TOKEN_FETCH_RETRIES {
            break val?;
        }
        match val {
            Ok(token) => break token,
            Err(e) => {
                error!("failed to get token: {}", e);
                sleep(TOKEN_DELAY).await;
            }
        }
    };

    info!("got a new token: {}", token.token);
    Ok(token)
}

/// Try to refresh the token until succeeds
#[tracing::instrument]
pub async fn get_token(orb_id: &str) -> Token {
    loop {
        match get_token_inner(
            orb_id,
            get_challenge_url().await,
            get_auth_token_url().await,
        )
        .await
        {
            Ok(token) => return token,
            Err(e) => {
                error!("failed to get token: {}", e);
                sleep(TOKEN_DELAY).await;
            }
        }
    }
}
