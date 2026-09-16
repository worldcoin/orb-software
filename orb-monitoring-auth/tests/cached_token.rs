#![cfg(feature = "testing")]

//! Verifies that monitoring auth serves a fresh cached token without refresh.

mod fixture;

use chrono::{DateTime, TimeDelta, Utc};
use color_eyre::Result;
use orb_monitoring_auth::server::{secure_storage::SecureStorage, Clock};
use serde_json::Value;

#[tokio::test]
async fn serves_fresh_cached_token_without_external_refresh() -> Result<()> {
    // Arrange
    let now = "2026-08-03T12:00:00Z".parse::<DateTime<Utc>>()?;
    let mut clock = Clock::faux();
    faux::when!(clock.now).then(move |_| now);

    let cached_token = fixture::token("cached-token", now + TimeDelta::days(181));
    let mut secure_storage = SecureStorage::faux();
    faux::when!(secure_storage.get).then(move |_| Ok(Some(cached_token.clone())));

    let fx = fixture::Fixture::new(clock, secure_storage)
        .await
        .run()
        .await;

    // Act
    let output = fx.run_client().await?;
    let response: Value = serde_json::from_slice(&output)?;
    let backend_requests = fx.backend.received_requests().await.unwrap_or_default();

    // Assert
    assert_eq!(response["monitoring_token"]["value"], "cached-token");
    assert_eq!(response["monitoring_token"]["error"], Value::Null);
    assert!(backend_requests.is_empty());

    fx.stop().await;

    Ok(())
}
