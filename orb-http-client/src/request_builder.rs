use color_eyre::Result;
use opentelemetry::propagation::Injector;
use reqwest::{header, Body, Client, Method, Response, Url};
use reqwest::header::{HeaderMap, HeaderValue};
use std::collections::HashMap;
use std::time::Instant;
use base64::Engine;
use tracing::{error, field, info, Span};
use tracing_opentelemetry::OpenTelemetrySpanExt;

/// A builder for constructing and sending a traced HTTP request.
#[derive(Clone)]
pub struct TracedRequestBuilder {
    client: Client,
    method: Method,
    url: Url,
    headers: HeaderMap,
    json_body: Option<serde_json::Value>,
    span: Span,
}

impl TracedRequestBuilder {
    /// Creates a new traced request builder.
    pub fn new(client: Client, method: Method, url: Url) -> Self {
        // Create a tracing span for this HTTP request
        let span = tracing::info_span!(
            "http.request",
            http.method = %method,
            http.url = %sanitize_url(&url)
        );

        TracedRequestBuilder {
            client,
            method,
            url,
            headers: HeaderMap::new(),
            json_body: None,
            span,
        }
    }

    /// Returns the URL used by this request.
    pub fn url(&self) -> &Url {
        &self.url
    }

    /// Replaces a path parameter placeholder (e.g. `{id}`) in the URL with the provided value.
    pub fn with_path_param(mut self, name: &str, value: &str) -> Self {
        let old_url = self.url.to_string();
        let new_url = old_url.replace(&format!("{{{}}}", name), value);

        self.url = Url::parse(&new_url)
            .unwrap_or_else(|_| panic!("Could not parse replaced URL {new_url}"));

        self
    }

    /// Adds a Bearer token header (e.g. `Authorization: Bearer <token>`).
    pub fn with_auth(mut self, token: &str) -> Self {
        self.headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", token))
                .expect("Invalid bearer token for header"),
        );
        self
    }

    /// Adds Basic authentication credentials.
    pub fn with_basic_auth(mut self, username: &str, password: &str) -> Self {
        let credentials = base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", username, password));
        self.headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Basic {}", credentials))
                .expect("Invalid basic auth credentials for header"),
        );
        self
    }

    /// Sets JSON body for the request.
    pub fn with_json<T: serde::Serialize>(mut self, json_body: &T) -> Self {
        self.json_body = Some(serde_json::to_value(json_body)
            .expect("Failed to serialize JSON body"));
        self
    }

    /// Sets a raw body for the request.
    pub fn with_body(self, body: impl Into<Body>) -> Self {
        // Start building the request immediately since Body isn't Clone
        self.client.request(self.method.clone(), self.url.clone())
            .body(body)
            .build()
            .expect("Failed to build request with body");
        self
    }

    /// Sends the HTTP request, injecting W3C trace context headers and recording a tracing span.
    pub async fn send(self) -> Result<Response> {
        let start = Instant::now();
        let _enter = self.span.enter();

        // Build the request with trace context
        let cx = Span::current().context();
        let mut injector = HeaderInjector::default();
        opentelemetry::global::get_text_map_propagator(|propagator| {
            propagator.inject_context(&cx, &mut injector);
        });

        // Start building the request
        let mut builder = self.client.request(self.method, self.url);

        // Add all headers
        for (k, v) in self.headers {
            if let Some(key) = k {
                builder = builder.header(key, v);
            }
        }

        // Add trace context headers
        for (k, v) in injector.0 {
            builder = builder.header(
                header::HeaderName::from_bytes(k.as_bytes())?,
                HeaderValue::from_str(&v)?,
            );
        }

        // Add body if present
        if let Some(json) = self.json_body {
            builder = builder.json(&json);
        }

        // Send the request
        let result = builder.send().await;
        let duration = start.elapsed();

        self.span.record("http.duration_ms", &field::display(duration.as_millis()));

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

        result.map_err(Into::into)
    }
}

// Helper types remain the same
#[derive(Default)]
struct HeaderInjector(HashMap<String, String>);

impl Injector for HeaderInjector {
    fn set(&mut self, key: &str, value: String) {
        self.0.insert(key.to_string(), value);
    }
}

fn sanitize_url(url: &Url) -> Url {
    let mut sanitized = url.clone();
    let _ = sanitized.set_username("");
    let _ = sanitized.set_password(None);
    sanitized.set_query(None);
    sanitized
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::StatusCode;
    use tokio;

    #[tokio::test]
    async fn test_sanitize_url() {
        let raw = Url::parse("https://user:pass@example.com/path?secret=123").unwrap();
        let san = sanitize_url(&raw);
        assert_eq!(san.username(), "");
        assert!(san.password().is_none());
        assert!(san.query().is_none());
    }

    #[tokio::test]
    async fn test_simple_request() -> Result<()> {
        let url = Url::parse("https://httpbin.org/get").unwrap();
        let trb = TracedRequestBuilder::new(reqwest::Client::new(), Method::GET, url);
        let resp = trb.send().await?;
        assert_eq!(resp.status(), StatusCode::OK);
        Ok(())
    }
}