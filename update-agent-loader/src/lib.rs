use std::{
    ffi::CString,
    io::{self, Write},
    os::fd::{FromRawFd, OwnedFd, RawFd},
    time::Duration,
};

use nix::{
    errno::Errno,
    sys::memfd::{memfd_create, MemFdCreateFlag},
};
use reqwest::blocking::Client;
use tracing::info;
use url::Url;

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("HTTP client error: {0}")]
    ClientError(#[from] reqwest::Error),
    
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    
    #[error("Memory file error: {0}")]
    MemFile(#[from] MemFileError),
}

#[derive(Debug, thiserror::Error)]
pub enum MemFileError {
    #[error("memfd_create failed: {0}")]
    MemfdCreate(#[from] Errno),
    
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
}

/// An in-memory file created with memfd_create
pub struct MemFile {
    fd: OwnedFd,
}

impl MemFile {
    /// Create a new empty memory file
    pub fn create() -> Result<Self, MemFileError> {
        // Create the memfd with a generic name
        let c_name = CString::new("memfile").expect("Static string should never fail");
        let raw_fd = memfd_create(&c_name, MemFdCreateFlag::empty())? as RawFd;
        let fd = unsafe { OwnedFd::from_raw_fd(raw_fd) };
        
        Ok(Self { fd })
    }
    
    /// Get a reference to the memory file's file descriptor
    fn fd(&self) -> &OwnedFd {
        &self.fd
    }
    
    /// Get the raw file descriptor as RawFd
    fn as_raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }
}

impl Write for MemFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let bytes_written = unsafe {
            libc::write(
                self.fd.as_raw_fd(), 
                buf.as_ptr() as *const libc::c_void, 
                buf.len()
            )
        };
        
        if bytes_written < 0 {
            return Err(io::Error::last_os_error());
        }
        
        Ok(bytes_written as usize)
    }
}

/// Downloads a file from the given URL directly into a MemFile
pub fn download_to_memfile(url: &Url) -> Result<MemFile, DownloadError> {
    let client = create_client()?;
    
    // Create an empty memory file
    let mut mem_file = MemFile::create()?;
    
    // Send request and stream the response directly to the memory file
    let mut response = client.get(url.clone()).send()?;
    
    // Copy the response body directly to the file
    let bytes_copied = io::copy(&mut response, &mut mem_file)?;
    
    info!("Downloaded {} bytes directly to memory file", bytes_copied);
    
    Ok(mem_file)
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
