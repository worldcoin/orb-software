#![cfg(feature = "testing")]

//! Verifies that monitoring auth replaces and serves a token nearing expiry.

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
async fn refreshes_and_replaces_token_nearing_expiry() -> Result<()> {
    // Arrange
    let now = "2026-08-03T12:00:00Z".parse::<DateTime<Utc>>()?;
    let mut clock = Clock::faux();
    faux::when!(clock.now).then(move |_| now);

    let expiring_token = fixture::token("expiring-token", now + TimeDelta::days(179));
    let persisted_tokens = Arc::new(Mutex::new(Vec::<Token>::new()));
    let persisted_tokens_for_put = Arc::clone(&persisted_tokens);
    let persisted = Arc::new(Notify::new());
    let persisted_for_put = Arc::clone(&persisted);
    let mut secure_storage = SecureStorage::faux();
    faux::when!(secure_storage.get).then(move |_| Ok(Some(expiring_token.clone())));
    faux::when!(secure_storage.put).then(move |token| {
        persisted_tokens_for_put
            .lock()
            .expect("persisted-token lock poisoned")
            .push(token.clone());
        persisted_for_put.notify_one();

        Ok(())
    });

    let replacement_token =
        fixture::token("replacement-token", now + TimeDelta::days(365));
    let expected_token = serde_json::to_value(&replacement_token)?;
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
        .respond_with(ResponseTemplate::new(200).set_body_json(&replacement_token))
        .expect(1)
        .mount(&fixture.backend)
        .await;
    let fx = fixture.run().await;

    // Act
    time::timeout(TimeDelta::seconds(5).to_std()?, persisted.notified())
        .await
        .expect("server did not persist the replacement token before timeout");
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
    assert_eq!(response["monitoring_token"]["value"], "replacement-token");
    assert_ne!(response["monitoring_token"]["value"], "expiring-token");
    assert_eq!(response["monitoring_token"]["error"], Value::Null);

    fx.stop().await;

    Ok(())
}
