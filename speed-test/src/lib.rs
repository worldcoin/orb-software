use color_eyre::Result;
use serde::Serialize;
use std::time::{Duration, Instant};

const CLOUDFLARE_UPLOAD_URL: &str = "https://speed.cloudflare.com/__up";
const CLOUDFLARE_DOWNLOAD_URL: &str = "https://speed.cloudflare.com/__down";

pub const DEFAULT_TEST_SIZE_BYTES: usize = 30_000_000;
const TIMEOUT_SECS: u64 = 30;

// Thresholds for connectivity quality (in Mbps)
const GOOD_DOWNLOAD_THRESHOLD: f64 = 20.0;
const GOOD_UPLOAD_THRESHOLD: f64 = 25.0;
const MEDIUM_DOWNLOAD_THRESHOLD: f64 = 10.0;
const MEDIUM_UPLOAD_THRESHOLD: f64 = 10.0;

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ConnectivityQuality {
    Good,
    Medium,
    Bad,
}

#[derive(Debug, Serialize)]
pub struct SpeedTestResults {
    pub connectivity: ConnectivityQuality,
    #[serde(serialize_with = "round_f64")]
    pub upload_mbps: f64,
    #[serde(serialize_with = "round_f64")]
    pub download_mbps: f64,
    pub upload_mb: f64,
    pub download_mb: f64,
    pub upload_duration_ms: u64,
    pub download_duration_ms: u64,
}

fn round_f64<S>(value: &f64, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_f64((value * 100.0).round() / 100.0)
}

struct ProbeResult {
    mbps: f64,
    elapsed: Duration,
}

/// Runs a network speed test using Cloudflare's speed test endpoints
///
/// # Arguments
/// * `test_size_bytes` - Size of data to transfer for each test
pub async fn run_speed_test(test_size_bytes: usize) -> Result<SpeedTestResults> {
    let timeout = Duration::from_secs(TIMEOUT_SECS);
    let client = reqwest::Client::builder().timeout(timeout).build()?;

    let upload_result = probe_upload(&client, test_size_bytes, timeout).await?;
    let download_result = probe_download(&client, test_size_bytes, timeout).await?;

    let connectivity =
        assess_connectivity_quality(upload_result.mbps, download_result.mbps);

    Ok(SpeedTestResults {
        connectivity,
        upload_mbps: upload_result.mbps,
        download_mbps: download_result.mbps,
        upload_mb: test_size_bytes as f64 / 1_000_000.0,
        download_mb: test_size_bytes as f64 / 1_000_000.0,
        upload_duration_ms: upload_result.elapsed.as_millis() as u64,
        download_duration_ms: download_result.elapsed.as_millis() as u64,
    })
}

fn assess_connectivity_quality(
    upload_mbps: f64,
    download_mbps: f64,
) -> ConnectivityQuality {
    if upload_mbps >= GOOD_UPLOAD_THRESHOLD && download_mbps >= GOOD_DOWNLOAD_THRESHOLD
    {
        ConnectivityQuality::Good
    } else if upload_mbps >= MEDIUM_UPLOAD_THRESHOLD
        && download_mbps >= MEDIUM_DOWNLOAD_THRESHOLD
    {
        ConnectivityQuality::Medium
    } else {
        ConnectivityQuality::Bad
    }
}

async fn probe_upload(
    client: &reqwest::Client,
    bytes: usize,
    timeout: Duration,
) -> Result<ProbeResult> {
    let payload = vec![0u8; bytes];

    let start = Instant::now();
    let resp = client
        .post(CLOUDFLARE_UPLOAD_URL)
        .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
        .timeout(timeout)
        .body(payload)
        .send()
        .await?
        .error_for_status()?;

    let _ = resp.bytes().await?;

    let elapsed = start.elapsed();
    let secs = elapsed.as_secs_f64().max(1e-6);
    let mbps = (bytes as f64 * 8.0) / secs / 1_000_000.0;

    Ok(ProbeResult { mbps, elapsed })
}

async fn probe_download(
    client: &reqwest::Client,
    bytes: usize,
    timeout: Duration,
) -> Result<ProbeResult> {
    let url = format!("{}?bytes={}", CLOUDFLARE_DOWNLOAD_URL, bytes);

    let start = Instant::now();
    let resp = client
        .get(&url)
        .timeout(timeout)
        .send()
        .await?
        .error_for_status()?;

    let downloaded = resp.bytes().await?;
    let elapsed = start.elapsed();

    let actual_bytes = downloaded.len();
    let secs = elapsed.as_secs_f64().max(1e-6);
    let mbps = (actual_bytes as f64 * 8.0) / secs / 1_000_000.0;

    Ok(ProbeResult { mbps, elapsed })
}
