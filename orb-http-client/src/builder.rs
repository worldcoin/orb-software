//! # orb-http-client
//!
//! Traced HTTP client built on [`reqwest`]. This crate automatically
//! injects OpenTelemetry trace context (W3C style) and uses pinned TLS certificates via
//! [`orb-security-utils`] to ensure secure, HTTPS-only connections by default.
//!
//! ## Key Features
//! - **Builder Pattern** for creating the traced client
//! - **Path Parameter** substitution (e.g. `/{id}` replaced with `my_id`)
//! - **Bearer & Basic Auth** support
//! - **JSON & raw body** support
//! - **HTTPS-only** and **no redirects** by default
//! - **Traced** requests (using [`tracing`] + [`opentelemetry`]), recording method, URL, status, duration, errors
//! - **Error Handling** with [`color-eyre`]
//!
//! ## Example
//! ```rust,no_run
//! use color_eyre::Result;
//! use std::time::Duration;
//! use orb_http_client::{TracedHttpClient, TracedHttpClientBuilder};
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     // Build a traced HTTP client, with pinned certs + TLS from orb-security-utils.
//!     let client = TracedHttpClientBuilder::new()
//!         .with_base_url("https://management.stage.orb.worldcoin.org")
//!         .with_timeout(Duration::from_secs(10))
//!         .build()?;
//!
//!     // Example usage: GET with path param + Bearer Auth
//!     let orb_id = "my_orb";
//!     let token = "my_bearer_token";
//!     let response = client
//!         .get("/api/v1/orbs/{id}/state")
//!         .with_path_param("id", orb_id)
//!         .with_auth(token)
//!         .send()
//!         .await?;
//!
//!     println!("Status: {}", response.status());
//!     println!("Body: {}", response.text().await?);
//!     Ok(())
//! }
//! ```

#![deny(missing_docs)]

use color_eyre::Result;
use opentelemetry::propagation::Injector;
use orb_security_utils::reqwest::http_client_builder;
use reqwest::{header, Body, Client, Method, Response, Url};
use serde::Serialize;
use std::collections::HashMap;
use std::fmt;
use std::time::{Duration, Instant};
use base64::Engine;
use reqwest::header::{HeaderMap, HeaderValue};
use tracing::{error, field, info, Span};
use tracing_opentelemetry::OpenTelemetrySpanExt;

/// A builder for configuring and constructing a [`TracedHttpClient`].
///
/// By default, HTTPS-only and pinned certificates are enforced, thanks to
/// `orb-security-utils` with the `"reqwest"` feature enabled. Redirects are
/// disabled for security reasons.
#[derive(Default, Debug)]
pub struct TracedHttpClientBuilder {
    base_url: Option<Url>,
    timeout: Option<Duration>,
    // Potential extension points: custom headers, per-request middleware, etc.
}

impl TracedHttpClientBuilder {
    /// Create a new blank builder with default settings.
    ///
    /// # Example
    /// ```rust
    /// use orb_http_client::TracedHttpClientBuilder;
    ///
    /// let builder = TracedHttpClientBuilder::new();
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets a base URL for all requests. Relative paths in request methods
    /// (e.g. `client.get("/some/path")`) will be joined to this base URL.
    ///
    /// # Panics
    /// If the provided string is not a valid URL.
    ///
    /// # Example
    /// ```rust
    /// # use orb_http_client::TracedHttpClientBuilder;
    /// let builder = TracedHttpClientBuilder::new()
    ///     .with_base_url("https://example.org");
    /// ```
    pub fn with_base_url(mut self, base: &str) -> Self {
        self.base_url = Some(
            Url::parse(base)
                .unwrap_or_else(|_| panic!("Invalid base URL provided: {}", base)),
        );
        self
    }

    /// Sets the maximum timeout for all requests.
    ///
    /// # Example
    /// ```rust
    /// # use orb_http_client::TracedHttpClientBuilder;
    /// # use std::time::Duration;
    /// let builder = TracedHttpClientBuilder::new()
    ///     .with_timeout(Duration::from_secs(5));
    /// ```
    pub fn with_timeout(mut self, duration: Duration) -> Self {
        self.timeout = Some(duration);
        self
    }

    /// Builds the [`TracedHttpClient`] with the pinned TLS configuration from
    /// `orb-security-utils`, respecting any settings (like `timeout`) applied here.
    ///
    /// # Errors
    /// If the underlying `reqwest::ClientBuilder` fails to build (rare), an error is returned.
    ///
    /// # Example
    /// ```rust,no_run
    /// # use color_eyre::Result;
    /// # use orb_http_client::TracedHttpClientBuilder;
    /// # use std::time::Duration;
    /// fn build_example() -> Result<()> {
    ///     let client = TracedHttpClientBuilder::new()
    ///         .with_base_url("https://api.example.org")
    ///         .with_timeout(Duration::from_secs(10))
    ///         .build()?;
    ///     Ok(())
    /// }
    /// ```
    pub fn build(self) -> Result<TracedHttpClient> {
        let mut builder = http_client_builder();
        if let Some(t) = self.timeout {
            builder = builder.timeout(t);
        }
        let client = builder.build()?;

        Ok(TracedHttpClient {
            client,
            base_url: self.base_url,
        })
    }
}

/// The main client that can build traced requests (`GET`, `POST`, etc.).
///
/// # Example
/// ```rust,no_run
/// # use color_eyre::Result;
/// # use std::time::Duration;
/// # use orb_http_client::{TracedHttpClient, TracedHttpClientBuilder};
/// # #[tokio::main]
/// # async fn main() -> Result<()> {
/// let client = TracedHttpClientBuilder::new()
///     .with_base_url("https://example.org")
///     .with_timeout(Duration::from_secs(10))
///     .build()?;
///
/// let resp = client.get("/hello").send().await?;
/// println!("status: {}", resp.status());
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct TracedHttpClient {
    /// The underlying Reqwest client with pinned TLS config.
    client: Client,
    /// If set, all relative paths get joined to this.
    base_url: Option<Url>,
}

impl TracedHttpClient {
    /// Creates a GET request builder for the given path.
    pub fn get(&self, path: &str) -> TracedRequestBuilder {
        self.request(Method::GET, path)
    }

    /// Creates a POST request builder for the given path.
    pub fn post(&self, path: &str) -> TracedRequestBuilder {
        self.request(Method::POST, path)
    }

    /// Creates a PUT request builder for the given path.
    pub fn put(&self, path: &str) -> TracedRequestBuilder {
        self.request(Method::PUT, path)
    }

    /// Creates a DELETE request builder for the given path.
    pub fn delete(&self, path: &str) -> TracedRequestBuilder {
        self.request(Method::DELETE, path)
    }

    /// Internal function to produce a [`TracedRequestBuilder`].
    fn request(&self, method: Method, path: &str) -> TracedRequestBuilder {
        let final_url = match &self.base_url {
            Some(base) => {
                // Attempt to join, else parse as absolute.
                base.join(path).unwrap_or_else(|_| {
                    Url::parse(path).unwrap_or_else(|_| {
                        panic!("Invalid path or URL: {}", path)
                    })
                })
            }
            None => {
                Url::parse(path).unwrap_or_else(|_| {
                    panic!("Invalid path or URL: {}", path)
                })
            }
        };

        TracedRequestBuilder::new(self.client.clone(), method, final_url)
    }
}

impl fmt::Debug for TracedHttpClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TracedHttpClient")
            .field("base_url", &self.base_url)
            .finish()
    }
}

/// A builder for constructing and sending an HTTP request with automatic
/// OpenTelemetry trace context injection.
///
/// This type is returned from methods like [`TracedHttpClient::get`].
pub struct TracedRequestBuilder {
    client: Client,
    method: Method,
    url: Url,

    // Because RequestBuilder is not Clone, we store data separately:
    headers: HeaderMap,
    json_body: Option<serde_json::Value>,
    raw_body: Option<Body>,

    span: Span,
}

impl TracedRequestBuilder {
    /// Creates a new traced request builder. Usually you won't call this directly;
    /// you'll get it from [`TracedHttpClient::get`], [`TracedHttpClient::post`], etc.
    pub fn new(client: Client, method: Method, url: Url) -> Self {
        let span = tracing::info_span!(
            "http.request",
            http.method = %method,
            http.url = %sanitize_url(&url)
        );

        // We start with empty custom headers, no body, etc.
        Self {
            client,
            method,
            url,
            headers: HeaderMap::new(),
            json_body: None,
            raw_body: None,
            span,
        }
    }

    /// Returns the underlying `Url` (for debugging, testing, etc.).
    pub fn url(&self) -> &Url {
        &self.url
    }

    /// Replaces a path parameter placeholder (e.g. `{id}`) in the URL with the provided value.
    ///
    /// This is a simple string replacement on the stored `url`. Example:
    /// ```rust
    /// # use orb_http_client::{TracedHttpClient, TracedHttpClientBuilder};
    /// # use color_eyre::Result;
    /// # fn example() -> Result<()> {
    /// let client = TracedHttpClientBuilder::new()
    ///     .with_base_url("https://example.com/api/")
    ///     .build()?;
    ///
    /// let builder = client.get("foo/{id}").with_path_param("id", "123");
    /// assert_eq!(builder.url().as_str(), "https://example.com/api/foo/123");
    /// # Ok(())
    /// # }
    /// ```
    pub fn with_path_param(mut self, name: &str, value: &str) -> Self {
        let old_url_str = self.url.to_string();
        let placeholder = format!("{{{}}}", name);
        let new_url_str = old_url_str.replace(&placeholder, value);

        let parsed = Url::parse(&new_url_str)
            .unwrap_or_else(|_| panic!("Could not parse replaced URL: {}", new_url_str));
        self.url = parsed;
        self
    }

    /// Adds a Bearer token header (e.g. `Authorization: Bearer <token>`).
    ///
    /// # Example
    /// ```rust
    /// # use orb_http_client::{TracedHttpClient, TracedHttpClientBuilder};
    /// # use color_eyre::Result;
    /// # async fn run() -> Result<()> {
    /// let client = TracedHttpClientBuilder::new().build()?;
    /// let resp = client
    ///     .get("https://httpbin.org/bearer")
    ///     .with_auth("my_token")
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn with_auth(mut self, token: &str) -> Self {
        let val = format!("Bearer {}", token);
        self.headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&val)
                .expect("Invalid bearer auth token for header value"),
        );
        self
    }

    /// Adds Basic authentication credentials (`Authorization: Basic <base64>`).
    ///
    /// # Example
    /// ```rust
    /// # use orb_http_client::{TracedHttpClient, TracedHttpClientBuilder};
    /// # use color_eyre::Result;
    /// # async fn run() -> Result<()> {
    /// let client = TracedHttpClientBuilder::new().build()?;
    /// let resp = client
    ///     .get("https://httpbin.org/basic-auth/username/password")
    ///     .with_basic_auth("username", "password")
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn with_basic_auth(mut self, username: &str, password: &str) -> Self {
        let credentials = base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", username, password));
        self.headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Basic {}", credentials))
                .expect("Invalid basic auth credentials for header"),
        );
        self
    }
    /// Sets JSON body for the request. Equivalent to `reqwest::RequestBuilder::json(...)`.
    ///
    /// # Example
    /// ```rust
    /// # use serde_json::json;
    /// # use orb_http_client::{TracedHttpClient, TracedHttpClientBuilder};
    /// # use color_eyre::Result;
    /// # async fn run() -> Result<()> {
    /// let client = TracedHttpClientBuilder::new().build()?;
    /// let body = json!({ "key": "value" });
    /// let resp = client
    ///     .post("https://httpbin.org/post")
    ///     .with_json(&body)
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn with_json<T: Serialize>(mut self, json_body: &T) -> Self {
        let value = serde_json::to_value(json_body)
            .expect("Failed to serialize JSON body");
        self.json_body = Some(value);
        self
    }

    /// Sets a raw body for the request. Equivalent to `reqwest::RequestBuilder::body(...)`.
    pub fn with_body(mut self, body: impl Into<Body>) -> Self {
        self.raw_body = Some(body.into());
        self
    }

    /// Sends the request asynchronously, injecting the W3C trace context into headers
    /// and recording standard HTTP attributes (status code, duration, etc.) in a tracing span.
    ///
    /// # Errors
    /// Returns a `color-eyre::Report` if sending fails or if the response cannot be read.
    ///
    /// # Example
    /// ```rust,no_run
    /// # use orb_http_client::{TracedHttpClient, TracedHttpClientBuilder};
    /// # use color_eyre::Result;
    /// # async fn run() -> Result<()> {
    /// let client = TracedHttpClientBuilder::new().build()?;
    /// let resp = client.get("https://httpbin.org/get").send().await?;
    /// println!("Status: {}", resp.status());
    /// # Ok(())
    /// # }
    /// ```
    pub async fn send(self) -> Result<Response> {
        let start = Instant::now();
        let _guard = self.span.enter();

        // Gather the current OpenTelemetry context (from this span),
        // so we can inject W3C trace headers.
        let cx = Span::current().context();
        let mut injector = HeaderInjector::default();
        opentelemetry::global::get_text_map_propagator(|prop| {
            prop.inject_context(&cx, &mut injector);
        });

        // Build a real `reqwest::RequestBuilder`.
        let mut rb = self.client.request(self.method.clone(), self.url.clone());

        // If the user set a JSON body, we apply it. Otherwise, if they set a raw body, we apply that.
        if let Some(json_val) = self.json_body {
            rb = rb.json(&json_val);
        } else if let Some(raw) = self.raw_body {
            rb = rb.body(raw);
        }

        // Merge in the custom headers (auth, etc.).
        for (k, v) in self.headers.iter() {
            rb = rb.header(k, v.clone());
        }

        // Add the W3C trace headers
        for (key, value) in injector.0 {
            rb = rb.header(
                header::HeaderName::from_bytes(key.as_bytes())?,
                HeaderValue::from_str(&value)?,
            );
        }

        // Send the actual HTTP request
        let result = rb.send().await;
        let duration = start.elapsed();
        self.span.record("http.duration_ms", &field::display(duration.as_millis()));

        // Log success/failure on the span
        match &result {
            Ok(response) => {
                let status = response.status();
                self.span.record("http.status_code", &field::display(status.as_u16()));
                if !status.is_success() {
                    error!(status = %status, "HTTP request did not succeed");
                } else {
                    info!(status = %status, "HTTP request succeeded");
                }
            }
            Err(e) => {
                error!(error = ?e, "HTTP request failed");
                self.span.record("error", &true);
            }
        }

        drop(_guard); // exit the span
        result.map_err(Into::into)
    }
}

/// A helper type to inject trace headers into the request.
#[derive(Default)]
struct HeaderInjector(HashMap<String, String>);

impl Injector for HeaderInjector {
    fn set(&mut self, key: &str, value: String) {
        self.0.insert(key.to_owned(), value);
    }
}


/// Sanitizes a URL for logging/tracing, removing user info (username/password),
/// query parameters, or anything else considered sensitive.
fn sanitize_url(url: &Url) -> Url {
    let mut cloned = url.clone();
    let _ = cloned.set_username("");
    let _ = cloned.set_password(None);
    cloned.set_query(None);
    cloned
}

// ─────────────────────────────────────────────────────────────────────────────
// TESTS
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use color_eyre::Report;
    use reqwest::StatusCode;
    use serde_json::json;

    #[test]
    fn test_builder_basics() -> Result<()> {
        let client = TracedHttpClientBuilder::new()
            .with_base_url("https://example.org")
            .with_timeout(Duration::from_secs(10))
            .build()?;
        assert_eq!(client.base_url.unwrap().as_str(), "https://example.org/");
        Ok(())
    }

    #[tokio::test]
    async fn test_path_param() -> Result<()> {
        let client = TracedHttpClientBuilder::new()
            .with_base_url("https://example.com/api/")
            .build()?;

        let builder = client.get("/foo/{id}").with_path_param("id", "42");
        assert_eq!(builder.url().as_str(), "https://example.com/api/foo/42");

        // We won't actually send to an existing server in this test, but let's ensure no panic:
        let _req = builder.with_auth("secret_token");
        Ok(())
    }

    #[tokio::test]
    async fn test_json_body() -> Result<()> {
        let client = TracedHttpClientBuilder::new().build()?;
        let builder = client
            .post("https://httpbin.org/post")
            .with_json(&json!({ "hello": "world" }));

        let resp = builder.send().await?;
        assert_eq!(resp.status(), StatusCode::OK);

        let json_resp: serde_json::Value = resp.json().await?;
        assert_eq!(json_resp["json"]["hello"], "world");
        Ok(())
    }

    #[tokio::test]
    async fn test_basic_auth() -> Result<()> {
        // httpbin has an endpoint that requires basic auth: /basic-auth/user/pass
        // If we provide the correct credentials, it returns 200.
        let client = TracedHttpClientBuilder::new().build()?;
        let builder = client
            .get("https://httpbin.org/basic-auth/user/pass")
            .with_basic_auth("user", "pass");

        let resp = builder.send().await?;
        assert_eq!(resp.status(), StatusCode::OK);
        Ok(())
    }
}
