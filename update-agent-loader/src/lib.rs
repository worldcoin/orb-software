// We're not using the never type directly since it's not stable yet
mod memfile;

use crate::memfile::ExecuteError;
use crate::memfile::{MemFile, MemFileError, Verified};
use std::{env, io};

use reqwest::blocking::Client;
use tracing::info;
use url::Url;

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("HTTP client error: {0}")]
    ClientError(#[from] reqwest::Error),

    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("MemFd error: {0}")]
    MemFile(#[from] MemFileError),

    #[error("Signature error: {0}")]
    SignatureError(String),
}

/// Downloads a file from the given URL directly into a MemFile
pub fn download(url: &Url) -> Result<MemFile<Verified>, DownloadError> {
    let client = create_client()?;

    // Create an empty memory file
    let mut mem_file = MemFile::create()?;

    // Record download start time
    let download_start = std::time::Instant::now();

    // Send request and stream the response directly to the memory file
    let mut response = client.get(url.clone()).send()?.error_for_status()?;

    info!(
        "Server responded with HTTP status code: {}",
        response.status()
    );

    // Copy the response body directly to the file
    let bytes_copied = io::copy(&mut response, &mut mem_file)?;

    let download_duration = download_start.elapsed();
    info!(
        "Downloaded {} bytes directly to memory file in {:.2?}",
        bytes_copied, download_duration
    );

    // Record signature verification start time
    let verification_start = std::time::Instant::now();

    // Verify signature, make executable, and seal
    let verified = match mem_file.verify_signature() {
        Ok(f) => f,
        Err(MemFileError::SignatureError(msg)) => {
            return Err(DownloadError::SignatureError(msg))
        }
        Err(e) => return Err(DownloadError::MemFile(e)),
    };

    let verification_duration = verification_start.elapsed();
    info!(
        "Signature verification completed in {:.2?}",
        verification_duration
    );
    Ok(verified)
}

/// Creates an HTTP client with security settings similar to the update-agent
fn create_client() -> Result<Client, DownloadError> {
    // Compile-time assertion to ensure allow_http feature isn't enabled in release mode
    #[cfg(all(feature = "allow_http", not(debug_assertions)))]
    compile_error!("The 'allow_http' feature cannot be enabled in release mode for security reasons");

    let builder = Client::builder()
        .tls_built_in_root_certs(true)
        .user_agent(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION")
        ));

    // In test mode, disable strict HTTPS and TLS requirements to allow for HTTP testing
    #[cfg(feature = "allow_http")]
    {
        tracing::debug!("allow_http mode: allowing HTTP URLs and not enforcing TLS requirements for testing");
        builder.build().map_err(DownloadError::ClientError)
    }

    // In normal mode, enforce strong security settings
    #[cfg(not(feature = "allow_http"))]
    {
        builder
            .https_only(true)
            .build()
            .map_err(DownloadError::ClientError)
    }
}

/// Downloads a file and executes it directly from memory
///
/// This function downloads a file from the given URL and then executes it
/// using fexecve, which allows execution directly from memory without
/// writing to disk. The current process will be replaced with the downloaded
/// executable if successful.
///
/// # Arguments
///
/// * `url` - The URL to download the executable from
/// * `args` - Command line arguments for the executable
///
/// # Returns
///
/// This function only returns on error. On success, the current process is replaced.
pub fn download_and_execute(url: &Url, args: &[&str]) -> Result<(), DownloadError> {
    let mem_file = download(url)?;

    // Log that we're about to execute the binary with the specified arguments
    info!(
        "Starting downloaded binary \"{}\" with {} arguments: {:?}",
        url,
        args.len(),
        args
    );

    // Execute the downloaded file
    mem_file.execute(args).map_err(|e| match e {
        ExecuteError::MemFile(e) => DownloadError::MemFile(e),
        ExecuteError::Io(e) => DownloadError::Io(e),
        ExecuteError::PermissionDenied => DownloadError::Io(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Permission denied",
        )),
        ExecuteError::Environment => DownloadError::Io(io::Error::new(
            io::ErrorKind::Other,
            "Failed to prepare execution environment",
        )),
    })
}
