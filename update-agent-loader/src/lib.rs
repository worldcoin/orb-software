use std::{
    ffi::CString,
    fs::File,
    io::{self, Seek, SeekFrom, Write},
    ops::Deref,
    os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
    path::Path,
    time::Duration,
};

use nix::{
    errno::Errno,
    sys::{
        memfd::{memfd_create, MemFdCreateFlag},
        mman::{mmap, munmap, MapFlags, ProtFlags},
        stat::fstat,
    },
};
use reqwest::blocking::Client;
use signify::{PublicKey, Signature};
use tracing::{info, warn};
use url::Url;

// Embedded public key for signature verification
// The actual key bytes would be included here
pub static EMBEDDED_PUBLIC_KEY: &[u8] = include_bytes!("../pubkey.pub");

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
    
    #[error("mmap failed: {0}")]
    Mmap(#[from] nix::Error),
    
    #[error("signature error: {0}")]
    Signature(String),
    
    #[error("fstat failed: {0}")]
    Fstat(nix::Error),
    
    #[error("zero-sized file")]
    ZeroSize,
}

/// An in-memory file created with memfd_create
pub struct MemFile {
    fd: OwnedFd,
}

/// A memory-mapped view of a MemFile that implements Deref to [u8]
pub struct MemFileMMap<'a> {
    mapped_ptr: *mut libc::c_void,
    size: usize,
    _file: &'a MemFile, // Keep a reference to the file to ensure it lives as long as the mapping
}

impl<'a> MemFileMMap<'a> {
    /// Create a new memory mapping from a MemFile
    pub fn new(file: &'a MemFile) -> Result<Self, MemFileError> {
        // Get the size of the file
        let size = file.size()?;
        if size == 0 {
            return Err(MemFileError::ZeroSize);
        }
        
        // Create a read-only memory mapping of the file
        let mapped_ptr = unsafe {
            mmap(
                None,
                size as usize,
                ProtFlags::PROT_READ,
                MapFlags::MAP_SHARED,
                file.as_raw_fd(),
                0,
            ).map_err(MemFileError::Mmap)?
        };
        
        Ok(Self {
            mapped_ptr,
            size: size as usize,
            _file: file,
        })
    }
}

impl<'a> Deref for MemFileMMap<'a> {
    type Target = [u8];
    
    fn deref(&self) -> &Self::Target {
        unsafe {
            std::slice::from_raw_parts(self.mapped_ptr as *const u8, self.size)
        }
    }
}

impl<'a> Drop for MemFileMMap<'a> {
    fn drop(&mut self) {
        unsafe {
            let _ = munmap(self.mapped_ptr, self.size);
        }
    }
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
    
    /// Get the size of the memory file
    fn size(&self) -> Result<u64, MemFileError> {
        let stat = fstat(self.fd.as_raw_fd())
            .map_err(MemFileError::Fstat)?;
        Ok(stat.st_size as u64)
    }
    
    /// Create a memory-mapped view of the file
    fn mmap(&self) -> Result<MemFileMMap<'_>, MemFileError> {
        MemFileMMap::new(self)
    }
    
    /// Verify the signature of the memory file
    fn verify_signature(&self, signature_path: impl AsRef<Path>) -> Result<bool, MemFileError> {
        // Create a memory-mapped view of the file
        let mmap = self.mmap()?;
        
        // Load the public key from embedded bytes
        let public_key = PublicKey::from_bytes(EMBEDDED_PUBLIC_KEY)
            .map_err(|e| MemFileError::Signature(format!("Failed to load embedded public key: {}", e)))?;
        
        // Load the signature
        let signature = Signature::from_file(signature_path)
            .map_err(|e| MemFileError::Signature(format!("Failed to load signature: {}", e)))?;
        
        // Verify the signature using the memory-mapped data
        let result = public_key.verify(&mmap, &signature)
            .map_err(|e| MemFileError::Signature(format!("Signature verification failed: {}", e)))?;
        
        Ok(result)
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

/// Downloads a file and verifies its signature using the embedded public key
pub fn download_and_verify(
    url: &Url, 
    signature_path: impl AsRef<Path>
) -> Result<MemFile, DownloadError> {
    let mem_file = download_to_memfile(url)?;
    
    // Verify the signature with the embedded public key
    if mem_file.verify_signature(&signature_path)
        .map_err(DownloadError::MemFile)? 
    {
        info!("Signature verification succeeded");
        Ok(mem_file)
    } else {
        info!("Signature verification failed");
        Err(DownloadError::MemFile(MemFileError::Signature(
            "Signature verification failed".into()
        )))
    }
}

/// Create a read-only memory mapping of a downloaded file
pub fn mmap_file(mem_file: &MemFile) -> Result<MemFileMMap<'_>, DownloadError> {
    mem_file.mmap().map_err(DownloadError::MemFile)
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
