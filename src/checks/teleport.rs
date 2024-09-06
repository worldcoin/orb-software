/// Teleport general health metrics URL.
/// `/readyz` is similar but includes more information about the process state.
const TELEPORT_DEFAULT_METRICS_URL: &str = "http://127.0.0.1:3000/healthz";

/// Error definition for teleport health check.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// Teleport status query failed.
    #[error("status query failed")]
    StatusQuery(#[source] reqwest::Error),
    /// Teleport server failure.
    #[error("server failure")]
    ServerFailure {
        status_code: reqwest::StatusCode,
        body: String,
    },
}

pub struct Teleport {
    metrics_url: String,
}

impl Teleport {
    /// Use Teleport default metrics URL.
    #[must_use]
    pub fn default() -> Self {
        Self {
            metrics_url: TELEPORT_DEFAULT_METRICS_URL.to_string(),
        }
    }

    /// Use a custom url for the metrics endpoint.
    /// Only used for testing purposes.
    #[cfg(test)]
    #[must_use]
    pub fn custom(url: String) -> Self {
        Self { metrics_url: url }
    }
}

impl super::Check for Teleport {
    type Error = Error;
    const NAME: &'static str = "teleport";

    /// The health check for teleport service.
    ///
    /// It checks the `TELEPORT_METRICS_URL` response and throws an error if the status code is anything other than OK.
    /// Also throws an error if the request to the endpoint fails.
    fn check(&self) -> Result<(), Self::Error> {
        let response = reqwest::blocking::get(self.metrics_url.as_str())
            .map_err(Error::StatusQuery)?;
        if !response.status().is_success() {
            return Err(Error::ServerFailure {
                status_code: response.status(),
                body: response
                    .text()
                    .unwrap_or("could not parse response body".to_string()),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use httpmock::prelude::*;
    use serde_json::{json, Value};

    use crate::{
        checks::{teleport, Check},
        Teleport,
    };
    // use super::super::run_check;

    // runs a local server which returns `status` and `body` and returns the server address url
    fn run_server(status: u16, json_body: Value, path: &str) -> String {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.path(path);
            then.status(status)
                .header("content-type", "application/json")
                .json_body(json_body);
        });
        server.url(path)
    }

    #[test]
    fn test_teleport_check_normal() {
        let body = json!({ "status": "ok" });
        let url = run_server(200, body, "/test_normal");
        Teleport::custom(url).run_check().unwrap();
    }

    #[test]
    fn test_teleport_check_server_error() {
        let body = json!({ "status": "internal server error" });
        let url = run_server(503, body, "/test_error");
        match Teleport::custom(url).run_check() {
            Ok(()) => panic!("expected error result but got Ok"),
            Err(error) => match error {
                teleport::Error::StatusQuery(_) => {
                    panic!("expected Error::ServerFailure but got Error::StatusQuery")
                }
                teleport::Error::ServerFailure { status_code, body } => {
                    assert_eq!(status_code, reqwest::StatusCode::SERVICE_UNAVAILABLE);
                    assert_eq!(body, body.to_string());
                }
            },
        }
    }

    #[test]
    fn test_teleport_check_query_error() {
        match Teleport::default().run_check() {
            Ok(()) => panic!("expected error result but got Ok"),
            Err(error) => {
                if !matches!(error, teleport::Error::StatusQuery(_)) {
                    panic!("got unexpected error variant {error}")
                }
            }
        }
    }
}
