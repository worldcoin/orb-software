//! # orb-http-client
//!
//! A traced HTTP client wrapper around [`reqwest`] that automatically propagates
//! OpenTelemetry trace context and reuses [`orb-security-utils`] for pinned TLS configuration.
//!
//! This crate provides:
//!  - A [`TracedHttpClient`] for making requests
//!  - A builder pattern ([`TracedHttpClientBuilder`])
//!  - Automatic W3C trace context propagation
//!  - Spans with key HTTP attributes (method, URL, status code, duration, errors)
//!  - Support for JSON, raw bodies, Basic Auth, and Bearer Auth
//!  - Security policies (HTTPS-only, disabled redirects, certificate pinning)
//!
//! ## Example
//! ```rust,no_run
//! use color_eyre::Result;
//! use orb_http_client::{TracedHttpClient, TracedHttpClientBuilder};
//! use std::time::Duration;
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     let client = TracedHttpClientBuilder::new()
//!         .with_base_url("https://management.stage.orb.worldcoin.org")
//!         .with_timeout(Duration::from_secs(30))
//!         .build()?;
//!
//!     let orb_id = "some_orb_id";
//!     let token = "some_bearer_token";
//!
//!     let response = client
//!         .get("/api/v1/orbs/{id}/state")
//!         .with_path_param("id", orb_id)
//!         .with_auth(token)
//!         .send()
//!         .await?;
//!
//!     println!("Response: {}", response.text().await?);
//!     Ok(())
//! }
//! ```

#![deny(missing_docs)]
mod builder;
mod client;
mod request_builder;

pub use builder::TracedHttpClientBuilder;
pub use client::TracedHttpClient;
pub use request_builder::TracedRequestBuilder;
