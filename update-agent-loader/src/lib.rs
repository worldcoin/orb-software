use std::time::Duration;

use reqwest::blocking::Client;
use reqwest::header::CONTENT_LENGTH;
use tracing::{info, warn};
use url::Url;

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("HTTP client error: {0}")]
    ClientError(#[from] reqwest::Error),
}

/// Downloads a file from the given URL and returns the data as a byte vector
pub fn download_file_in_memory(url: &Url) -> Result<Vec<u8>, DownloadError> {
    let client = create_client()?;
    
    // Send initial request to get content length
    let response = client.get(url.clone()).send()?;
    
    // Get content length if available
    let content_length = if let Some(content_length) = response.headers().get(CONTENT_LENGTH) {
        match content_length.to_str() {
            Ok(length_str) => match length_str.parse::<u64>() {
                Ok(length) => Some(length),
                Err(_) => {
                    warn!("Invalid content length value: {}", length_str);
                    None
                }
            },
            Err(_) => None,
        }
    } else {
        None
    };

    if let Some(length) = content_length {
        info!("Downloading file of size: {} bytes", length);
    } else {
        info!("Downloading file of unknown size");
    }

    // Download the actual file data
    let bytes = response.bytes()?;
    
    info!("Download complete. Received {} bytes", bytes.len());
    
    Ok(bytes.to_vec())
}

/// Creates an HTTP client with security settings similar to the update-agent
fn create_client() -> Result<Client, DownloadError> {
    Client::builder()
        .tls_built_in_root_certs(true)
        .min_tls_version(reqwest::tls::Version::TLS_1_3)
        .redirect(reqwest::redirect::Policy::none())
        .https_only(true)
        .user_agent(concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(DownloadError::ClientError)
}