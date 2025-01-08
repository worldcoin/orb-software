use crate::request_builder::TracedRequestBuilder;
use reqwest::{Client, Method, Url};
use std::fmt;

/// A traced HTTP client that automatically injects OpenTelemetry trace context
/// and records spans for each request.
#[derive(Clone)]
pub struct TracedHttpClient {
    /// The underlying `reqwest` client with pinned TLS configuration.
    pub(crate) client: Client,
    /// An optional base URL to be used for requests.
    pub(crate) base_url: Option<Url>,
}

impl TracedHttpClient {
    /// Creates a new GET request builder for the given path.
    pub fn get(&self, path: &str) -> TracedRequestBuilder {
        self.request(Method::GET, path)
    }

    /// Creates a new POST request builder for the given path.
    pub fn post(&self, path: &str) -> TracedRequestBuilder {
        self.request(Method::POST, path)
    }

    /// Creates a new PUT request builder for the given path.
    pub fn put(&self, path: &str) -> TracedRequestBuilder {
        self.request(Method::PUT, path)
    }

    /// Creates a new DELETE request builder for the given path.
    pub fn delete(&self, path: &str) -> TracedRequestBuilder {
        self.request(Method::DELETE, path)
    }

    /// A general function to create a new [`TracedRequestBuilder`] with a tracing span.
    fn request(&self, method: Method, path: &str) -> TracedRequestBuilder {
        // Try to join the path to the base URL if present, otherwise parse as a full URL.
        let url = match &self.base_url {
            Some(base) => {
                base.join(path).unwrap_or_else(|_| {
                    // Fallback: parse `path` independently
                    Url::parse(path).unwrap_or_else(|_| {
                        panic!("Invalid path provided: {}", path);
                    })
                })
            }
            None => {
                Url::parse(path).unwrap_or_else(|_| {
                    panic!("Invalid path provided: {}", path);
                })
            }
        };

        TracedRequestBuilder::new(self.client.clone(), method, url)
    }
}

impl fmt::Debug for TracedHttpClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TracedHttpClient")
            .field("base_url", &self.base_url)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::StatusCode;
    use std::time::Duration;
    use tokio::runtime::Runtime;

    #[test]
    fn test_get_request_construction() {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            let client = crate::builder::TracedHttpClientBuilder::new()
                .with_timeout(Duration::from_secs(5))
                .build()
                .unwrap();

            let request_builder = client.get("https://httpbin.org/get");
            // Just ensuring it doesn't panic:
            let _ = request_builder;
        });
    }

    #[test]
    fn test_join_with_base_url() {
        let client = crate::builder::TracedHttpClientBuilder::new()
            .with_base_url("https://example.com/api/")
            .build()
            .unwrap();

        let builder = client.get("v1/test");
        let url_str = builder.url().to_string();
        assert_eq!(url_str, "https://example.com/api/v1/test");
    }

    #[test]
    fn test_fallback_invalid_join() {
        let client = crate::builder::TracedHttpClientBuilder::new()
            .with_base_url("https://example.com/api/")
            .build()
            .unwrap();

        // "://" in "blah://" might make the join fail, so we parse `blah://path` directly.
        let builder = client.get("blah://path");
        let url_str = builder.url().to_string();
        assert_eq!(url_str, "blah://path/");
    }

    #[test]
    fn test_send_real_request() {
        // Real network call test to httpbin.org (remove or mock for offline tests).
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let client = crate::builder::TracedHttpClientBuilder::new()
                .with_timeout(Duration::from_secs(5))
                .build()
                .unwrap();

            let response = client
                .get("https://httpbin.org/get")
                .send()
                .await
                .expect("Failed to send request");

            assert_eq!(response.status(), StatusCode::OK);
        });
    }
}
