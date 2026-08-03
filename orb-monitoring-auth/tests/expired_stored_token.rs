#![cfg(feature = "testing")]

//! Verifies that expired stored monitoring tokens are cleared and never
//! exposed through the monitoring-auth client boundary.

mod fixture;

use chrono::{DateTime, TimeDelta, Utc};
use color_eyre::eyre::{eyre, Result};
use orb_monitoring_auth::server::{secure_storage::SecureStorage, Clock};
use serde_json::Value;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

#[tokio::test]
async fn clears_and_does_not_serve_expired_stored_token() -> Result<()> {
    // Arrange
    let now = "2026-08-03T12:00:00Z".parse::<DateTime<Utc>>()?;
    let mut clock = Clock::faux();
    faux::when!(clock.now).then(move |_| now);

    let expired_token = fixture::token("expired-token", now - TimeDelta::seconds(1));
    let clear_calls = Arc::new(AtomicUsize::new(0));
    let clear_calls_for_clear = Arc::clone(&clear_calls);
    let mut secure_storage = SecureStorage::faux();
    faux::when!(secure_storage.get).then(move |_| Ok(Some(expired_token.clone())));
    faux::when!(secure_storage.clear).then(move |_| {
        clear_calls_for_clear.fetch_add(1, Ordering::SeqCst);

        Ok(())
    });

    let fx = fixture::Fixture::new(clock, secure_storage)
        .await
        .run()
        .await;

    // Act
    let output = fx.run_client().await?;
    let response: Value = serde_json::from_slice(&output)?;

    // Assert
    assert_eq!(
        clear_calls.load(Ordering::SeqCst),
        1,
        "an expired stored token should be cleared exactly once"
    );
    assert_eq!(
        response["monitoring_token"]["value"],
        Value::Null,
        "an expired stored token must not be served"
    );
    assert_eq!(
        response["monitoring_token"]["error"], "there is no token available",
        "the client should report the existing missing-token error"
    );

    fx.stop().await;

    Ok(())
}

#[tokio::test]
async fn does_not_serve_expired_token_when_clearing_fails() -> Result<()> {
    // Arrange
    let now = "2026-08-03T12:00:00Z".parse::<DateTime<Utc>>()?;
    let mut clock = Clock::faux();
    faux::when!(clock.now).then(move |_| now);

    let expired_token = fixture::token("expired-token", now);
    let clear_calls = Arc::new(AtomicUsize::new(0));
    let clear_calls_for_clear = Arc::clone(&clear_calls);
    let mut secure_storage = SecureStorage::faux();
    faux::when!(secure_storage.get).then(move |_| Ok(Some(expired_token.clone())));
    faux::when!(secure_storage.clear).then(move |_| {
        clear_calls_for_clear.fetch_add(1, Ordering::SeqCst);

        Err(eyre!("failed to clear expired token"))
    });

    let fx = fixture::Fixture::new(clock, secure_storage)
        .await
        .run()
        .await;

    // Act
    let output = fx.run_client().await?;
    let response: Value = serde_json::from_slice(&output)?;

    // Assert
    assert_eq!(
        clear_calls.load(Ordering::SeqCst),
        1,
        "clearing should be attempted exactly once"
    );
    assert_eq!(
        response["monitoring_token"]["value"],
        Value::Null,
        "a clear failure must not make an expired token available"
    );
    assert_eq!(
        response["monitoring_token"]["error"], "there is no token available",
        "the client should report the existing missing-token error"
    );

    fx.stop().await;

    Ok(())
}
