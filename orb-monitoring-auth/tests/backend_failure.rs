#![cfg(feature = "testing")]

//! Verifies that monitoring auth preserves empty state when token refresh is
//! rejected by the backend.

mod fixture;

use chrono::{DateTime, Utc};
use color_eyre::Result;
use orb_monitoring_auth::{
    server::{secure_storage::SecureStorage, Clock},
    Token,
};
use serde_json::Value;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::time;
use wiremock::{
    matchers::{header, method, path},
    Mock, ResponseTemplate,
};

#[tokio::test]
async fn preserves_empty_state_when_backend_rejects_refresh() -> Result<()> {
    // Arrange
    let now = "2026-08-03T12:00:00Z".parse::<DateTime<Utc>>()?;
    let mut clock = Clock::faux();
    faux::when!(clock.now).then(move |_| now);

    let persisted_tokens = Arc::new(Mutex::new(Vec::<Token>::new()));
    let persisted_tokens_for_put = Arc::clone(&persisted_tokens);
    let mut secure_storage = SecureStorage::faux();
    faux::when!(secure_storage.get).then(|_| Ok(None));
    faux::when!(secure_storage.put).then(move |token| {
        persisted_tokens_for_put
            .lock()
            .expect("persisted-token lock poisoned")
            .push(token.clone());

        Ok(())
    });

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
        .respond_with(ResponseTemplate::new(503).set_body_string("backend unavailable"))
        .expect(1)
        .mount(&fixture.backend)
        .await;
    let fx = fixture.run().await;

    // Act
    let backend_requests = time::timeout(Duration::from_secs(5), async {
        loop {
            let requests = fx.backend.received_requests().await.unwrap_or_default();
            if !requests.is_empty() {
                break requests;
            }

            time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("backend did not receive the refresh request before timeout");
    let output = fx.run_client().await?;
    let response: Value = serde_json::from_slice(&output)?;
    let persisted_tokens = persisted_tokens
        .lock()
        .expect("persisted-token lock poisoned")
        .clone();

    // Assert
    assert_eq!(
        backend_requests.len(),
        1,
        "refresh should make exactly one request before the restart backoff"
    );
    assert!(
        persisted_tokens.is_empty(),
        "a rejected refresh must not persist a token"
    );
    assert_eq!(
        response["monitoring_token"]["value"],
        Value::Null,
        "the socket should continue to expose an empty token state"
    );
    assert_eq!(
        response["monitoring_token"]["error"], "there is no token available",
        "the client should use its existing missing-token response"
    );

    fx.stop().await;

    Ok(())
}
