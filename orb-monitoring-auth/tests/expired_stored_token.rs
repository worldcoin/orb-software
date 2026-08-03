#![cfg(feature = "testing")]

//! Verifies that monitoring auth never exposes an expired persisted token,
//! whether it is already expired at startup or expires while the server runs.

mod fixture;

use chrono::{DateTime, TimeDelta, Utc};
use color_eyre::eyre::{eyre, Result};
use orb_monitoring_auth::server::{secure_storage::SecureStorage, Clock};
use serde_json::Value;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, RwLock,
};

#[tokio::test]
async fn stops_serving_cached_token_after_it_expires_while_running() -> Result<()> {
    // Arrange
    let initial_now = "2026-08-03T12:00:00Z".parse::<DateTime<Utc>>()?;
    let expiry = initial_now + TimeDelta::days(181);
    let current_time = Arc::new(RwLock::new(initial_now));
    let current_time_for_clock = Arc::clone(&current_time);
    let mut clock = Clock::faux();
    faux::when!(clock.now).then(move |_| {
        current_time_for_clock
            .read()
            .expect("current-time lock poisoned")
            .to_owned()
    });

    let cached_token = fixture::token("cached-token", expiry);
    let mut secure_storage = SecureStorage::faux();
    faux::when!(secure_storage.get).then(move |_| Ok(Some(cached_token.clone())));

    let fx = fixture::Fixture::new(clock, secure_storage)
        .await
        .run()
        .await;

    // Act
    let valid_output = fx.run_client().await?;
    let valid_response: Value = serde_json::from_slice(&valid_output)?;
    *current_time.write().expect("current-time lock poisoned") = expiry;
    let expired_output = fx.run_client().await?;
    let expired_response: Value = serde_json::from_slice(&expired_output)?;

    // Assert
    assert_eq!(valid_response["monitoring_token"]["value"], "cached-token");
    assert_eq!(valid_response["monitoring_token"]["error"], Value::Null);
    assert_eq!(
        expired_response["monitoring_token"]["value"],
        Value::Null,
        "a token must not be served at its expiry instant"
    );
    assert_eq!(
        expired_response["monitoring_token"]["error"],
        "there is no token available"
    );

    fx.stop().await;

    Ok(())
}

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
