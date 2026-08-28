use crate::stream::Event;
use chrono::{DateTime, Utc};
use color_eyre::Result;
use derive_more::From;
use eyre::Context;
use orb_dogd::MetricEmitter;
use orb_info::{OrbId, OrbJabilId, OrbName};
use rand::Rng;
use reqwest::{
    blocking::{Client, Response},
    Url,
};
use serde::{Deserialize, Serialize};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tracing::{error, instrument};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OesStatusApiV2 {
    pub orb_id: Option<String>,
    pub orb_name: Option<String>,
    pub jabil_id: Option<String>,
    pub version: Option<VersionApiV2>,
    pub timestamp: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oes: Option<Vec<Event>>,
    pub oes_cached: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionApiV2 {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_release: Option<String>,
}

#[derive(Debug, From)]
pub enum Err {
    MissingAttestToken,
    #[from]
    Other(color_eyre::Report),
}

/// Shared handle onto the most recently fetched attest token, updated by
/// `token_watcher`'s polling thread.
pub type SharedToken = Arc<Mutex<String>>;

const MAX_ATTEMPTS: u32 = 3;

#[derive(Clone)]
pub struct StatusClient<M> {
    client: Client,
    endpoint: Url,
    orb_id: String,
    orb_name: String,
    jabil_id: String,
    orb_os_version: String,
    token: SharedToken,
    min_retry_interval: Duration,
    max_retry_interval: Duration,
    metrics: M,
}

#[bon::bon]
impl<M: MetricEmitter + Clone> StatusClient<M> {
    #[builder]
    pub fn new(
        metrics: M,
        orb_id: OrbId,
        orb_name: OrbName,
        jabil_id: OrbJabilId,
        orb_os_version: String,
        endpoint: Url,
        req_timeout: Duration,
        min_req_retry_interval: Duration,
        max_req_retry_interval: Duration,
        token: SharedToken,
    ) -> Result<Self> {
        let client = orb_security_utils::reqwest::blocking::client_builder()
            .timeout(req_timeout)
            .user_agent("orb-oes")
            .build()
            .wrap_err("failed to build reqwest client")?;

        Ok(Self {
            client,
            endpoint,
            orb_id: orb_id.to_string(),
            orb_name: orb_name.to_string(),
            jabil_id: jabil_id.to_string(),
            orb_os_version,
            token,
            min_retry_interval: min_req_retry_interval,
            max_retry_interval: max_req_retry_interval,
            metrics,
        })
    }

    /// Sends `payload` to the backend, retrying transient failures
    /// (network errors, 5xx) with exponential backoff and jitter, bounded to
    /// [`MAX_ATTEMPTS`] tries.
    #[instrument(skip_all, err(Debug))]
    pub fn req(&self, payload: OesStatusApiV2) -> std::result::Result<Response, Err> {
        let token = self
            .token
            .lock()
            .map_err(|_| eyre::eyre!("token lock poison"))?
            .clone();

        if token.is_empty() {
            return Err(Err::MissingAttestToken);
        }

        let req = OesStatusApiV2 {
            orb_id: Some(self.orb_id.clone()),
            orb_name: Some(self.orb_name.clone()),
            jabil_id: Some(self.jabil_id.clone()),
            version: Some(VersionApiV2 {
                current_release: Some(self.orb_os_version.clone()),
            }),
            timestamp: Utc::now(),
            ..payload
        };

        let mut backoff = self.min_retry_interval;
        let mut last_err = None;

        for attempt in 0..MAX_ATTEMPTS {
            let start = Instant::now();
            let outcome = self
                .client
                .post(self.endpoint.clone())
                .json(&req)
                .basic_auth(&self.orb_id, Some(token.clone()))
                .send()
                .map_err(color_eyre::Report::from)
                .and_then(|res| {
                    let status = res.status();
                    if status.is_server_error() {
                        Err(color_eyre::eyre::eyre!("backend returned {status}"))
                    } else {
                        Ok(res)
                    }
                });
            let elapsed = start.elapsed().as_millis();
            let ok_tag = if outcome.is_ok() {
                "ok:true"
            } else {
                "ok:false"
            };

            let _ = self
                .metrics
                .count("orb.platform.oes.client_req", 1, [ok_tag]);
            let _ = self.metrics.dist(
                "orb.platform.oes.client_req_duration",
                elapsed as f64,
                [ok_tag],
            );

            match outcome {
                Ok(res) => return Ok(res),
                Err(e) => {
                    error!(attempt, "OES backend request failed: {e:?}");
                    last_err = Some(e);
                }
            }

            if attempt + 1 < MAX_ATTEMPTS {
                std::thread::sleep(jittered(backoff));
                backoff = (backoff * 2).min(self.max_retry_interval);
            }
        }

        Err(Err::Other(last_err.unwrap_or_else(|| {
            color_eyre::eyre::eyre!("request failed with no error recorded")
        })))
    }
}

/// Adds up to 20% random jitter on top of `base`, so retrying clients don't
/// all wake up in lockstep.
fn jittered(base: Duration) -> Duration {
    let jitter_factor = rand::thread_rng().gen_range(1.0..1.2);

    base.mul_f64(jitter_factor)
}
