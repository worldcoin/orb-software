use base64::prelude::*;
use color_eyre::eyre::{Context, Result};
use flate2::write::GzEncoder;
use flate2::Compression;
use orb_attest_dbus::AuthTokenManagerProxy;
use reqwest::multipart::{Form, Part};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tar::Builder;
use uuid::Uuid;

const CLOUDFLARE_UPLOAD_URL: &str = "https://speed.cloudflare.com/__up";
const CLOUDFLARE_DOWNLOAD_URL: &str = "https://speed.cloudflare.com/__down";
const CLOUDFLARE_TIMEOUT_SECS: u64 = 30;

const DATA_BACKEND_BASE_URL: &str = "https://data.stage.orb.worldcoin.org";
const PCP_TIMEOUT_SECS: u64 = 60;

/// Backend expects exactly this string. Change with caution
const TEST_SIGNUP_ID: &str = "test-signup-00000000";

// Network quality thresholds based on real-world scenarios
// Upload speed thresholds (in Mbps)
const EXCELLENT_UPLOAD_THRESHOLD: f64 = 20.0;
const GOOD_UPLOAD_THRESHOLD: f64 = 5.0;
const TYPICAL_UPLOAD_THRESHOLD: f64 = 1.0;
const POOR_UPLOAD_THRESHOLD: f64 = 0.5;

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ConnectivityQuality {
    Excellent,
    Good,
    Typical,
    Poor,
    Worst,
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

#[derive(Debug, Serialize)]
pub struct PcpSpeedTestResults {
    pub connectivity: ConnectivityQuality,
    #[serde(serialize_with = "round_f64")]
    pub upload_mbps: f64,
    pub upload_mb: f64,
    pub upload_duration_ms: u64,
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

#[derive(Debug, Deserialize)]
struct PresignedUrlResponse {
    url: String,
    fields: Option<HashMap<String, String>>,
}

#[derive(Debug, Serialize)]
struct PackageRequest<'a> {
    #[serde(rename = "OrbID")]
    orb_id: &'a str,
    #[serde(rename = "SessionId")]
    session_id: &'a str,
    #[serde(rename = "Checksum")]
    checksum: &'a str,
    #[serde(rename = "IDCommitment")]
    id_commitment: &'a str,
}

pub async fn run_speed_test(test_size_bytes: usize) -> Result<SpeedTestResults> {
    let timeout = Duration::from_secs(CLOUDFLARE_TIMEOUT_SECS);
    let client = reqwest::Client::builder().timeout(timeout).build()?;

    let upload_result = probe_upload(&client, test_size_bytes, timeout).await?;
    let download_result = probe_download(&client, test_size_bytes, timeout).await?;

    let min_speed = upload_result.mbps.min(download_result.mbps);
    let connectivity = assess_connectivity_quality(min_speed);

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

fn assess_connectivity_quality(speed_mbps: f64) -> ConnectivityQuality {
    if speed_mbps >= EXCELLENT_UPLOAD_THRESHOLD {
        ConnectivityQuality::Excellent
    } else if speed_mbps >= GOOD_UPLOAD_THRESHOLD {
        ConnectivityQuality::Good
    } else if speed_mbps >= TYPICAL_UPLOAD_THRESHOLD {
        ConnectivityQuality::Typical
    } else if speed_mbps >= POOR_UPLOAD_THRESHOLD {
        ConnectivityQuality::Poor
    } else {
        ConnectivityQuality::Worst
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

pub async fn run_pcp_speed_test(
    test_size_bytes: usize,
    orb_id: &orb_info::orb_id::OrbId,
    dbus_connection: &zbus::Connection,
    num_uploads: usize,
) -> Result<PcpSpeedTestResults> {
    let num_uploads = num_uploads.max(1);

    let token = get_auth_token(dbus_connection)
        .await
        .context("Failed to get authentication token")?;

    let session_id = Uuid::new_v4().to_string();

    let pcp_data = create_mock_pcp_targz(test_size_bytes)
        .context("Failed to create mock PCP data")?;

    let mut hasher = Sha256::new();
    hasher.update(&pcp_data);
    let checksum = BASE64_STANDARD.encode(hasher.finalize());

    let actual_bytes = pcp_data.len();
    let mut total_mbps = 0.0;
    let mut total_duration_ms = 0u64;

    for _ in 0..num_uploads {
        let presigned_response =
            request_presigned_url(orb_id.as_str(), &session_id, &checksum, &token)
                .await
                .context("Failed to request presigned URL")?;

        let elapsed = upload_to_presigned_url(
            &presigned_response.url,
            presigned_response.fields,
            pcp_data.clone(),
        )
        .await
        .context("Failed to upload PCP data")?;

        let secs = elapsed.as_secs_f64().max(1e-6);
        let mbps = (actual_bytes as f64 * 8.0) / secs / 1_000_000.0;
        let duration_ms = elapsed.as_millis() as u64;

        total_mbps += mbps;
        total_duration_ms += duration_ms;
    }

    let avg_mbps = total_mbps / num_uploads as f64;
    let avg_duration_ms = total_duration_ms / num_uploads as u64;

    Ok(PcpSpeedTestResults {
        connectivity: assess_connectivity_quality(avg_mbps),
        upload_mbps: avg_mbps,
        upload_mb: actual_bytes as f64 / 1_000_000.0,
        upload_duration_ms: avg_duration_ms,
    })
}

async fn get_auth_token(dbus_connection: &zbus::Connection) -> Result<String> {
    let proxy = AuthTokenManagerProxy::new(dbus_connection)
        .await
        .context("Failed to create AuthTokenManager proxy")?;

    let token = proxy
        .token()
        .await
        .context("Failed to get token from AuthTokenManager")?;

    Ok(token)
}

/// Create a mock PCP tar.gz
///
/// Uses a mix of random and structured data to simulate realistic compression ratios.
fn create_mock_pcp_targz(target_size: usize) -> Result<Vec<u8>> {
    use rand::Rng;

    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    {
        let mut tar = Builder::new(&mut encoder);
        let mut rng = rand::thread_rng();

        // 1. Simulated encrypted data (high entropy, ~80% of data)
        // This should not compress well
        let encrypted_size = (target_size as f64 * 0.8) as usize;
        let mut encrypted_data = vec![0u8; encrypted_size];
        rng.fill(&mut encrypted_data[..]);

        let mut header = tar::Header::new_gnu();
        header.set_size(encrypted_size as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, "tier1_encrypted.bin", &encrypted_data[..])
            .context("Failed to append encrypted data")?;

        // 2. Simulated JSON metadata (structured text, ~20% of data)
        // This should compress well
        let json_size = target_size - encrypted_size;
        let json_pattern = br#"{"key":"value","timestamp":1234567890,"data":"#;
        let mut json_data = Vec::with_capacity(json_size);
        while json_data.len() < json_size {
            json_data.extend_from_slice(json_pattern);
        }
        json_data.truncate(json_size);

        let mut header = tar::Header::new_gnu();
        header.set_size(json_size as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, "metadata.json", &json_data[..])
            .context("Failed to append metadata")?;

        tar.finish().context("Failed to finish tar archive")?;
    }

    encoder
        .finish()
        .context("Failed to finish gzip compression")
}

async fn request_presigned_url(
    orb_id: &str,
    session_id: &str,
    checksum: &str,
    token: &str,
) -> Result<PresignedUrlResponse> {
    let endpoint = format!(
        "{}/api/v3/signups/{}/package",
        DATA_BACKEND_BASE_URL, TEST_SIGNUP_ID
    );

    let request_body = PackageRequest {
        orb_id,
        session_id,
        checksum,
        id_commitment: "mock_id_commitment",
    };

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(PCP_TIMEOUT_SECS))
        .build()?;

    let response = client
        .post(&endpoint)
        .basic_auth(orb_id, Some(token))
        .json(&request_body)
        .send()
        .await
        .context("Failed to request presigned URL")?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| String::from("<unable to read body>"));
        return Err(color_eyre::eyre::eyre!(
            "Presigned URL request failed with status {}: {}",
            status,
            body
        ));
    }

    response
        .json::<PresignedUrlResponse>()
        .await
        .context("Failed to parse presigned URL response")
}

async fn upload_to_presigned_url(
    presigned_url: &str,
    fields: Option<HashMap<String, String>>,
    data: Vec<u8>,
) -> Result<Duration> {
    let file_part = Part::bytes(data)
        .file_name("package.tar.gz")
        .mime_str("application/octet-stream")?;

    let mut form = fields
        .unwrap_or_default()
        .into_iter()
        .fold(Form::new(), |form, (key, value)| form.text(key, value));

    form = form.part("file", file_part);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(PCP_TIMEOUT_SECS))
        .build()?;

    let start = Instant::now();
    client
        .post(presigned_url)
        .multipart(form)
        .send()
        .await
        .context("Failed to upload to presigned URL")?
        .error_for_status()
        .context("Upload returned error status")?;

    let elapsed = start.elapsed();

    Ok(elapsed)
}
