use std::{
    fmt,
    io::Write,
    process::{Command, Stdio},
};

use data_encoding::BASE64;
use ring::{digest, digest::digest};
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use serde_with::{base64::Base64, serde_as};
use tokio::{
    fs::read_to_string,
    time::{self, sleep},
};
use tracing::{error, event, info, warn, Level};
use url::Url;

/// Path to persistent token, don't use it directly, use `Token::from_usr_persistent()` instead
#[cfg(test)]
const STATIC_TOKEN_PATH: &str = "./test_token";
#[cfg(not(test))]
const STATIC_TOKEN_PATH: &str = "/usr/persistent/token";
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
    #[error("failed to initialized HTTP client: {}", .0)]
    HTTPClientInitFailed(#[source] crate::client::Error),
    #[error("HTTP challenge request failed: {}", .0)]
    PostFailed(#[source] reqwest::Error),
    #[error("failed to parse JSON response: {}", .0)]
    JsonParseFailed(#[source] reqwest::Error),
    #[error("Server returned status {} and error: {}", .0, .1)]
    ServerReturnedError(reqwest::StatusCode, String),
}

#[derive(Debug, thiserror::Error)]
pub enum SignError {
    #[error("no sign binary is found on system")]
    NoSignBinary,
    #[error("failed to spawn sign tool: {}", .0)]
    SpawnFailed(#[source] std::io::Error),
    #[error("failed to write to sign tool stdin: {}", .0)]
    WriteFailed(#[source] std::io::Error),
    #[error("failed to read from to sign tool stdout: {}", .0)]
    ReadFailed(#[source] std::io::Error),
    #[error("sign tool failed to sign the challenge")]
    SignFailed,
    #[error("SE is not provisioned")]
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
    #[error("incomprehensible output: {}, original output: \"{}\"", .0, .1)]
    BadOutput(#[source] data_encoding::DecodeError, String),
}

#[derive(Debug, thiserror::Error)]
pub enum TokenError {
    #[error("failed to initialized HTTP client: {}", .0)]
    HTTPClientInitFailed(#[source] crate::client::Error),
    #[error("post request to the server failed: {}", .0)]
    PostFailed(#[source] reqwest::Error),
    #[error("server returned error status code {0} with body \"{1}\"")]
    ServerReturnedError(reqwest::StatusCode, String),
    #[error("failed to parse JSON response: {}", .0)]
    JsonParseFailed(#[source] reqwest::Error),
    #[error("token field is empty in the response")]
    EmptyResponse,
}

#[derive(Debug, thiserror::Error)]
pub enum RefreshTokenError {
    #[error("failed to fetch challenge: {}", .0)]
    ChallengeError(#[source] ChallengeError),
    #[error("failed to sign challenge: {}", .0)]
    SignError(#[source] SignError),
    #[error("failed to fetch token: {}", .0)]
    TokenError(#[source] TokenError),
    #[error("challenge token expired before we could fetch a token")]
    ChallengeExpired,
    #[error("encountered panic while singing the challenge: {}", .0)]
    JoinError(#[source] tokio::task::JoinError),
}

/// helper for concealing part of a secret from the log.
/// splits the secret in three parts and print the first and last part
fn format_secret(val: &str) -> String {
    let len = val.len();
    let begin = &val[..len / 3];
    let end = &val[2 * len / 3..];
    format!("{begin:?}..{end:?}")
}

#[serde_as]
#[derive(Deserialize, Clone)]
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

        // TODO check errkind
        if !output.status.success() {
            event!(Level::ERROR, sign_tool_log = ?sign_tool_log, orb_sign_attestation_success = output.status.success(), orb_sign_attestation_code = output.status.code());
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
            signature: BASE64.decode(&output.stdout).map_err(|e| {
                SignError::BadOutput(
                    e,
                    String::from_utf8_lossy(&output.stdout).to_string(),
                )
            })?,
        })
    }
}

/// To hide the value of the challenge from the log, print only beginning and the end of it.
impl fmt::Debug for Challenge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "challenge: {}, duration: {}s",
            format_secret(&BASE64.encode(&self.challenge)),
            self.duration.as_secs()
        )
    }
}

#[serde_as]
#[derive(Serialize)]
pub struct Signature {
    #[serde(rename = "Signature")]
    #[serde_as(as = "Base64")]
    signature: Vec<u8>,
}

/// To hide the value of the signature from the log, print only beginning and the end of it.
impl fmt::Debug for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "signature: {}",
            format_secret(&BASE64.encode(&self.signature))
        )
    }
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
#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct Token {
    /// token value
    #[serde(rename = "token")]
    pub token: SecretString,
    /// token validity period in seconds
    #[serde_as(as = "serde_with::DurationSeconds<u64>")]
    duration: std::time::Duration,
    /// token expiration time in server time
    #[serde(rename = "expiryTime")]
    expiry_time: String,
    /// local time when the token was fetched
    #[serde(skip, default = "time::Instant::now")]
    start_time: time::Instant,
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
                    warn!(error=?e, "failed to read response body: {}", e);
                    String::new()
                }
            };
            Err(TokenError::ServerReturnedError(status, msg))
        } else {
            match resp.json::<Token>().await {
                Ok(token) => Ok(token),
                Err(e) => {
                    error!(error=?e, "failed to parse token response: {}", e);
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

    /// Return a token with infinite expiration date and value from
    /// `STATIC_TOKEN_PATH` file.
    ///
    /// # Errors
    /// - if failed to read the file
    pub async fn from_usr_persistent() -> std::io::Result<Self> {
        let token = SecretString::from(
            read_to_string(STATIC_TOKEN_PATH).await?.trim().to_owned(),
        );
        Ok(Self {
            token,
            duration: std::time::Duration::MAX,
            expiry_time: String::new(),
            start_time: tokio::time::Instant::now(),
        })
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

    info!("got a new token: {token:?}");
    Ok(token)
}

/// Try to refresh the token until succeeds
///
/// Panics
///
/// if fails to construct API URL
#[tracing::instrument]
pub async fn get_token(orb_id: &str, base_url: &Url) -> Token {
    let tokenchallenge_url = base_url.join("tokenchallenge").unwrap();
    let token_url = base_url.join("token").unwrap();

    loop {
        match get_token_inner(orb_id, &tokenchallenge_url, &token_url).await {
            Ok(token) => return token,
            Err(e) => {
                error!("failed to get token: {}", e);
                sleep(TOKEN_DELAY).await;
            }
        }
    }
}

#[cfg(test)]
mod test {
    use std::os::unix::fs::PermissionsExt;

    use data_encoding::BASE64;
    use secrecy::ExposeSecret;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    const MOCK_ORB_SIGN_ATTESTATION: &str = r#"#!/bin/sh
echo -n dmFsaWRzaWduYXR1cmU=
"#;
    // A happy path
    #[tokio::test]
    async fn get_token() {
        crate::logging::init();

        let mock_server = MockServer::start().await;

        let orb_id = "TEST_ORB";
        let challenge =
            "challenge_token_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
        let challenge_response = serde_json::json!({
            "challenge": BASE64.encode(challenge.as_ref()),
            "duration": 3600,
            "expiryTime": "is not used by client",
        });

        Mock::given(method("POST"))
            .and(path("/api/v1/tokenchallenge"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&challenge_response))
            .mount(&mock_server)
            .await;

        let server_token =
            "token_CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC";
        let token_response = serde_json::json!({
            "token": server_token,
            "duration": 36000,
            "expiryTime": "is not used by client",
        });

        Mock::given(method("POST"))
            .and(path("/api/v1/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&token_response))
            .mount(&mock_server)
            .await;

        let base_url: url::Url = mock_server
            .uri()
            .parse::<url::Url>()
            .unwrap()
            .join("/api/v1/")
            .unwrap();
        let token_challenge = base_url.join("tokenchallenge").unwrap();

        // 1. get challenge
        let challenge =
            crate::remote_api::Challenge::request(orb_id, &token_challenge).await;

        assert!(challenge.is_ok());
        let challenge = challenge.unwrap();
        let clone_of_challenge = challenge.clone();

        // Create a mock signing script orb-sign-attestation that returns pre-defined challenge and
        // add it to PATH
        let mut path = std::env::var("PATH").unwrap();
        let temp_dir = tempfile::tempdir().unwrap();
        let script = temp_dir.path().join("orb-sign-attestation");
        std::fs::write(&script, MOCK_ORB_SIGN_ATTESTATION).unwrap();
        std::fs::set_permissions(script, std::fs::Permissions::from_mode(0o755))
            .unwrap();
        path.push(':');
        path.push_str(temp_dir.path().to_str().unwrap());
        std::env::set_var("PATH", path);

        // 2. sign challenge
        let signature = tokio::task::spawn_blocking(move || clone_of_challenge.sign())
            .await
            .unwrap();

        assert!(
            signature.is_ok(),
            "failed to sign challenge: {}",
            signature.unwrap_err()
        );

        // 3. get token
        let token = crate::remote_api::Token::request(
            &base_url.join("token").unwrap(),
            orb_id,
            &challenge,
            &signature.unwrap(),
        )
        .await;
        assert!(token.is_ok());
        assert_eq!(server_token, token.unwrap().token.expose_secret());
    }
}
