#![cfg(feature = "testing")]

//! Verifies that monitoring auth fetches, persists, and serves a token when
//! secure storage is empty.

mod fixture;

use chrono::{DateTime, TimeDelta, Utc};
use color_eyre::Result;
use orb_monitoring_auth::{
    server::{secure_storage::SecureStorage, Clock},
    Token,
};
use serde_json::Value;
use std::sync::{Arc, Mutex};
use tokio::{sync::Notify, time};
use wiremock::{
    matchers::{header, method, path},
    Mock, ResponseTemplate,
};

#[tokio::test]
async fn fetches_persists_and_serves_token_when_storage_is_empty() -> Result<()> {
    // Arrange
    let now = "2026-08-03T12:00:00Z".parse::<DateTime<Utc>>()?;
    let mut clock = Clock::faux();
    faux::when!(clock.now).then(move |_| now);

    let persisted_tokens = Arc::new(Mutex::new(Vec::<Token>::new()));
    let persisted_tokens_for_put = Arc::clone(&persisted_tokens);
    let persisted = Arc::new(Notify::new());
    let persisted_for_put = Arc::clone(&persisted);
    let mut secure_storage = SecureStorage::faux();
    faux::when!(secure_storage.get).then(|_| Ok(None));
    faux::when!(secure_storage.put).then(move |token| {
        persisted_tokens_for_put
            .lock()
            .expect("persisted-token lock poisoned")
            .push(token.clone());
        persisted_for_put.notify_one();

        Ok(())
    });

    let fetched_token = fixture::token("fetched-token", now + TimeDelta::days(365));
    let expected_token = serde_json::to_value(&fetched_token)?;
    let fixture = fixture::Fixture::new(clock, secure_storage)
        .await
        .attest_token("attestation-token")
        .await;
    Mock::given(method("GET"))
        .and(path("/monitoring-token"))
        .and(header(
            "authorization",
            "Basic YmJhODViYWE6YXR0ZXN0YXRpb24tdG9rZW4=",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(&fetched_token))
        .expect(1)
        .mount(&fixture.backend)
        .await;
    let fx = fixture.run().await;

    // Act
    time::timeout(TimeDelta::seconds(5).to_std()?, persisted.notified())
        .await
        .expect("server did not persist the fetched token before timeout");
    let output = fx.run_client().await?;
    let response: Value = serde_json::from_slice(&output)?;
    let backend_requests = fx.backend.received_requests().await.unwrap_or_default();
    let persisted_tokens = persisted_tokens
        .lock()
        .expect("persisted-token lock poisoned")
        .iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()?;

    // Assert
    assert_eq!(backend_requests.len(), 1);
    assert_eq!(persisted_tokens, vec![expected_token]);
    assert_eq!(response["monitoring_token"]["value"], "fetched-token");
    assert_eq!(response["monitoring_token"]["error"], Value::Null);

    fx.stop().await;

    Ok(())
}
