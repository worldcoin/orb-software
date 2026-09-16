#![cfg(feature = "testing")]

//! Verifies that monitoring auth replaces an expiring token only with a backend
//! token that satisfies the shared freshness policy.

mod fixture;

use chrono::{DateTime, TimeDelta, Utc};
use color_eyre::Result;
use orb_monitoring_auth::{
    server::{secure_storage::SecureStorage, Clock},
    Token,
};
use serde_json::Value;
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::{sync::Notify, time};
use wiremock::{
    matchers::{header, method, path},
    Mock, ResponseTemplate,
};

#[tokio::test]
async fn rejects_expired_backend_token_when_storage_is_empty() -> Result<()> {
    // Arrange
    let now = "2026-08-03T12:00:00Z".parse::<DateTime<Utc>>()?;
    let mut clock = Clock::faux();
    faux::when!(clock.now).then(move |_| now);

    let put_calls = Arc::new(AtomicUsize::new(0));
    let put_calls_for_put = Arc::clone(&put_calls);
    let mut secure_storage = SecureStorage::faux();
    faux::when!(secure_storage.get).then(|_| Ok(None));
    faux::when!(secure_storage.put).then(move |_| {
        put_calls_for_put.fetch_add(1, Ordering::SeqCst);

        Ok(())
    });

    let expired_token = fixture::token("expired-backend-token", now);
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
        .respond_with(ResponseTemplate::new(200).set_body_json(&expired_token))
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

    // Assert
    assert_eq!(backend_requests.len(), 1);
    assert_eq!(
        put_calls.load(Ordering::SeqCst),
        0,
        "an expired backend token must not be persisted"
    );
    assert_eq!(response["monitoring_token"]["value"], Value::Null);
    assert_eq!(
        response["monitoring_token"]["error"],
        "there is no token available"
    );

    fx.stop().await;

    Ok(())
}

#[tokio::test]
async fn preserves_cached_token_when_backend_returns_token_inside_refresh_window(
) -> Result<()> {
    // Arrange
    let now = "2026-08-03T12:00:00Z".parse::<DateTime<Utc>>()?;
    let mut clock = Clock::faux();
    faux::when!(clock.now).then(move |_| now);

    let cached_token = fixture::token("cached-token", now + TimeDelta::days(179));
    let put_calls = Arc::new(AtomicUsize::new(0));
    let put_calls_for_put = Arc::clone(&put_calls);
    let mut secure_storage = SecureStorage::faux();
    faux::when!(secure_storage.get).then(move |_| Ok(Some(cached_token.clone())));
    faux::when!(secure_storage.put).then(move |_| {
        put_calls_for_put.fetch_add(1, Ordering::SeqCst);

        Ok(())
    });

    let insufficiently_fresh_token =
        fixture::token("backend-token", now + TimeDelta::days(179));
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
        .respond_with(
            ResponseTemplate::new(200).set_body_json(&insufficiently_fresh_token),
        )
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

    // Assert
    assert_eq!(backend_requests.len(), 1);
    assert_eq!(
        put_calls.load(Ordering::SeqCst),
        0,
        "a backend token inside the refresh window must not be persisted"
    );
    assert_eq!(response["monitoring_token"]["value"], "cached-token");
    assert_eq!(response["monitoring_token"]["error"], Value::Null);

    fx.stop().await;

    Ok(())
}

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
