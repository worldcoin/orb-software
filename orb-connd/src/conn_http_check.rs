use color_eyre::Result;
use serde::Serializer;
use std::time::{Duration, Instant};

#[derive(Debug, serde::Serialize)]
pub struct ConnHttpCheck {
    #[serde(serialize_with = "serialize_status_code")]
    pub status: reqwest::StatusCode,
    pub location: Option<String>,
    pub nm_status: Option<String>,
    pub content_length: Option<String>,
    pub elapsed: Duration,
}

impl ConnHttpCheck {
    /// Does a connectivity check equivalent to the NetworkManager one
    /// iface will default to default route if `None` is passed as arg
    pub async fn run(connectivity_uri: &str, iface: Option<&str>) -> Result<Self> {
        let client = if let Some(iface) = iface {
            reqwest::Client::builder().interface(iface)
        } else {
            reqwest::Client::builder()
        }
        .timeout(Duration::from_secs(5))
        .build()?;

        let start = Instant::now();
        let res = client.get(connectivity_uri).send().await?;
        let elapsed = start.elapsed();

        let check = Self {
            status: res.status(),
            location: res
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string),
            nm_status: res
                .headers()
                .get("x-networkmanager-status")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string),
            content_length: res
                .headers()
                .get("content-length")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string),
            elapsed,
        };

        Ok(check)
    }
}

fn serialize_status_code<S: Serializer>(
    status: &reqwest::StatusCode,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_u16(status.as_u16())
}
