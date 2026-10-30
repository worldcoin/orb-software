use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};

use eyre::{ensure, Result, WrapErr};
use orb_info::OrbId;
use reqwest::{
    header::{HeaderValue, ETAG, IF_NONE_MATCH, RETRY_AFTER},
    Client, StatusCode, Url,
};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Map, Value};

pub struct Snapshot {
    pub etag: Option<HeaderValue>,
    pub sections: Arc<Map<String, Value>>,
}

pub enum Fetch {
    Updated(Snapshot),
    Unchanged,
    RetryAfter(Duration),
}

pub async fn fetch(
    client: &Client,
    endpoint: &Url,
    orb_id: &OrbId,
    token: &SecretString,
    cached: Option<&Snapshot>,
) -> Result<Fetch> {
    let mut request = client
        .get(endpoint.clone())
        .basic_auth(orb_id.to_string(), Some(token.expose_secret()));
    if let Some(etag) = cached.and_then(|snapshot| snapshot.etag.as_ref()) {
        request = request.header(IF_NONE_MATCH, etag);
    }

    let response = request.send().await.map_err(reqwest::Error::without_url)?;
    let status = response.status();
    if status == StatusCode::NOT_MODIFIED {
        ensure!(
            cached.is_some_and(|snapshot| snapshot.etag.is_some()),
            "backend returned 304 without a cached configuration"
        );
        return Ok(Fetch::Unchanged);
    }
    if matches!(
        status,
        StatusCode::TOO_MANY_REQUESTS | StatusCode::SERVICE_UNAVAILABLE
    ) && let Some(delay) = response
        .headers()
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(retry_after)
    {
        tracing::warn!(%status, retry_secs = delay.as_secs(), "backend requested a later poll");
        return Ok(Fetch::RetryAfter(delay));
    }
    ensure!(
        status == StatusCode::OK,
        "configuration backend returned {status}"
    );

    let etag = response.headers().get(ETAG).cloned();
    let body = response
        .bytes()
        .await
        .map_err(reqwest::Error::without_url)?;
    let sections = serde_json::from_slice::<Map<String, Value>>(&body)
        .wrap_err("configuration must be a JSON object")?;

    // Only replace the cached ETag after parsing its corresponding body.
    Ok(Fetch::Updated(Snapshot {
        etag,
        sections: Arc::new(sections),
    }))
}

fn retry_after(value: &str) -> Option<Duration> {
    value
        .parse::<u64>()
        .map(Duration::from_secs)
        .ok()
        .or_else(|| {
            httpdate::parse_http_date(value)
                .ok()
                .map(|at| at.duration_since(SystemTime::now()).unwrap_or_default())
        })
        .map(|delay| delay.min(Duration::from_secs(300)))
}
