// We're not using the never type directly since it's not stable yet

use std::marker::PhantomData;
use std::{
    env,
    ffi::{c_void, CString},
    io::{self, Write},
    num::NonZeroUsize,
    ops::Deref,
    os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
    time::Duration,
};

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use nix::{
    errno::Errno,
    sys::{
        memfd::{memfd_create, MemFdCreateFlag},
        mman::{mmap, munmap, MapFlags, ProtFlags},
        stat::fstat,
    },
    unistd::{fexecve, ftruncate, write},
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

    #[error("Signature error: {0}")]
    SignatureError(String),
}

#[derive(Debug, thiserror::Error)]
pub enum ExecuteError {
    #[error("Failed to execute: {0}")]
    Io(#[from] io::Error),

    #[error("Permission denied")]
    PermissionDenied,

    #[error("Memory file error: {0}")]
    MemFile(#[from] MemFileError),

    #[error("Failed to prepare execution environment")]
    Environment,
}

#[derive(Debug)]
pub enum MemFileError {
    MemfdCreate(Errno),
    Io(io::Error),
    Mmap(nix::Error),
    Fstat(nix::Error),
}

impl std::fmt::Display for MemFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MemfdCreate(e) => write!(f, "memfd_create failed: {}", e),
            Self::Io(e) => write!(f, "I/O error: {}", e),
            Self::Mmap(e) => write!(f, "mmap failed: {}", e),
            Self::Fstat(e) => write!(f, "fstat failed: {}", e),
        }
    }
}

impl std::error::Error for MemFileError {}

impl From<Errno> for MemFileError {
    fn from(err: Errno) -> Self {
        MemFileError::MemfdCreate(err)
    }
}

impl From<io::Error> for MemFileError {
    fn from(err: io::Error) -> Self {
        MemFileError::Io(err)
    }
}

// We can't use From<nix::Error> because it conflicts with From<Errno>
// since Errno is a variant of nix::Error

/// An in-memory file created with memfd_create
pub struct MemFile<S: MemFileState> {
    fd: OwnedFd,
    _marker: PhantomData<S>,
}

pub enum Unverified {}
pub enum Verified {}

pub trait MemFileState {}
impl MemFileState for Unverified {}
impl MemFileState for Verified {}

/// A memory-mapped view of a MemFile that implements Deref to [u8]
pub struct MemFileMMap<'a> {
    mapped_ptr: *mut c_void,
    size: usize,
    _file: &'a MemFile<Unverified>, // Keep a reference to the file to ensure it lives as long as the mapping
}

impl<'a> MemFileMMap<'a> {
    /// Create a new memory mapping from a MemFile
    pub fn new(file: &'a MemFile<Unverified>) -> Result<Self, MemFileError> {
        // Get the size of the file
        let size = file.size()?;

        let non_zero_size = NonZeroUsize::new(size as usize).ok_or_else(|| {
            MemFileError::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Cannot map zero-sized file",
            ))
        })?;

        // Create a read-only memory mapping of the file
        let mapped_ptr = unsafe {
            mmap(
                None,
                non_zero_size,
                ProtFlags::PROT_READ,
                MapFlags::MAP_PRIVATE,
                file.as_raw_fd(),
                0,
            )
            .map_err(MemFileError::Mmap)?
        };

        Ok(Self {
            mapped_ptr,
            size: size as usize,
            _file: file,
        })
    }
}

impl Deref for MemFileMMap<'_> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        unsafe { std::slice::from_raw_parts(self.mapped_ptr as *const u8, self.size) }
    }
}

impl Drop for MemFileMMap<'_> {
    fn drop(&mut self) {
        unsafe {
            let _ = munmap(self.mapped_ptr, self.size);
        }
    }
}

impl<S: MemFileState> MemFile<S> {
    /// Get the size of the memory file
    fn size(&self) -> Result<u64, MemFileError> {
        match fstat(self.fd.as_raw_fd()) {
            Ok(stat) => Ok(stat.st_size as u64),
            Err(e) => Err(MemFileError::Fstat(e)),
        }
    }
    /// Get the raw file descriptor as RawFd
    pub fn as_raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }
}

impl MemFile<Unverified> {
    /// Create a new empty memory file with close-on-exec flag
    pub fn create() -> Result<Self, MemFileError> {
        // Create the memfd with a generic name and close-on-exec flag
        let c_name = CString::new("memfile").expect("Static string should never fail");
        let raw_fd = memfd_create(&c_name, MemFdCreateFlag::MFD_CLOEXEC)? as RawFd;
        let fd = unsafe { OwnedFd::from_raw_fd(raw_fd) };

        Ok(Self {
            fd,
            _marker: PhantomData,
        })
    }

    /// Create a memory-mapped view of the file
    fn mmap(&self) -> Result<MemFileMMap<'_>, MemFileError> {
        MemFileMMap::new(self)
    }

    /// Truncate the file to the specified size
    fn truncate(&self, size: u64) -> Result<(), MemFileError> {
        // Truncate the file using ftruncate
        ftruncate(self.fd.as_raw_fd(), size as i64)
            .map_err(|e| MemFileError::Io(io::Error::new(io::ErrorKind::Other, e)))?;

        Ok(())
    }
    /// Verifies the signature in the file
    ///
    /// This reads the last 4 bytes to determine the signature size,
    /// extracts and verifies the signature, then truncates the file
    /// to remove the signature.
    pub fn verify_signature(self) -> Result<MemFile<Verified>, MemFileError> {
        // Create a memory-mapped view of the file
        let mmap = self.mmap()?;

        // Check if the file is large enough to contain at least the signature size (4 bytes)
        if mmap.len() < 4 {
            return Err(MemFileError::Io(io::Error::new(
                io::ErrorKind::InvalidData,
                "File too small to contain signature size",
            )));
        }

        // Read the signature size from the last 4 bytes (u32, little-endian)
        let sig_size_bytes = &mmap[mmap.len() - 4..];
        let sig_size = u32::from_le_bytes([
            sig_size_bytes[0],
            sig_size_bytes[1],
            sig_size_bytes[2],
            sig_size_bytes[3],
        ]) as usize;

        // Ensure the signature size is valid (ed25519 signatures are 64 bytes)
        if sig_size != 64 {
            return Err(MemFileError::Io(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Invalid signature size: {} (expected 64)", sig_size),
            )));
        }

        // Check if file is large enough to contain the signature + size field
        if mmap.len() < sig_size + 4 {
            return Err(MemFileError::Io(io::Error::new(
                io::ErrorKind::InvalidData,
                "File too small to contain signature",
            )));
        }

        // Extract the signature
        let sig_data = &mmap[mmap.len() - sig_size - 4..mmap.len() - 4];

        // Copy signature to a separate buffer with the correct size for ed25519-dalek
        let mut sig_bytes = [0u8; 64];
        sig_bytes.copy_from_slice(sig_data);

        // Create a signature from the bytes
        let signature = Signature::from_bytes(&sig_bytes);

        // Parse the public key (using the hardcoded key for now)
        let public_key = match VerifyingKey::from_bytes(PUBLIC_KEY_BYTES) {
            Ok(key) => key,
            Err(e) => {
                return Err(MemFileError::Io(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Failed to parse public key: {}", e),
                )));
            }
        };

        // The data to verify is everything except the signature and its size
        let data = &mmap[..mmap.len() - sig_size - 4];

        // Truncate the file to remove the signature and size field first
        let new_size = self.size()? - (sig_size as u64) - 4;
        self.truncate(new_size)?;

        // Verify the signature
        if let Err(e) = public_key.verify(data, &signature) {
            return Err(MemFileError::Io(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Signature verification failed: {}", e),
            )));
        }

        drop(mmap);

        Ok(MemFile {
            fd: self.fd,
            _marker: PhantomData,
        })
    }
}

impl MemFile<Verified> {
    /// Execute the memory file using fexecve
    ///
    /// This will replace the current process with the executed program.
    /// The file descriptor must point to an executable file.
    ///
    /// # Arguments
    ///
    /// * `args` - Command line arguments for the executable
    ///
    /// # Returns
    ///
    /// This function only returns on error. On success, the current process is replaced.
    pub fn execute(&self, args: &[&str]) -> Result<(), ExecuteError> {
        // Verify that the file is not empty
        let size = self.size().map_err(ExecuteError::MemFile)?;
        if size == 0 {
            return Err(ExecuteError::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Cannot execute zero-sized file",
            )));
        }

        // Prepare arguments for fexecve
        let mut c_args: Vec<CString> = Vec::with_capacity(args.len() + 1);

        // Add the program name as first argument (args[0])
        c_args.push(CString::new("memfile").map_err(|_| ExecuteError::Environment)?);

        // Add the rest of the arguments
        for arg in args {
            let c_arg = CString::new(arg as &str)
                .map_err(|_| ExecuteError::Environment)?;
            c_args.push(c_arg);
        }

        // We don't need pointers for nix::unistd::fexecve

        // Execute the file
        info!("Executing memory file with fd: {}", self.as_raw_fd());
        // Convert the args to a slice of CStr references as required by nix::unistd::fexecve
        let args_cstr: Vec<&std::ffi::CStr> =
            c_args.iter().map(|arg| arg.as_c_str()).collect();

        // Get current environment variables and convert to CString
        let env_vars: Vec<CString> = env::vars()
            .map(|(key, val)| CString::new(format!("{}={}", key, val)))
            .filter_map(Result::ok)
            .collect();

        // Convert env vars to CStr references
        let env_cstr: Vec<&std::ffi::CStr> =
            env_vars.iter().map(|var| var.as_c_str()).collect();

        // Use nix::unistd::fexecve with the current environment
        let result = fexecve(self.fd.as_raw_fd(), &args_cstr, &env_cstr);

        // If we get here, fexecve failed
        match result {
            Ok(_) => {
                // This should never happen as fexecve shouldn't return on success
                Err(ExecuteError::Io(io::Error::new(
                    io::ErrorKind::Other,
                    "fexecve returned unexpectedly",
                )))
            }
            Err(nix::Error::EPERM) => {
                // Permission denied
                Err(ExecuteError::PermissionDenied)
            }
            Err(e) => {
                // Convert other nix errors to io::Error
                Err(ExecuteError::Io(io::Error::new(
                    io::ErrorKind::Other,
                    format!("fexecve failed: {}", e),
                )))
            }
        }
    }
}

impl Write for MemFile<Unverified> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match write(self.fd.as_raw_fd(), buf) {
            Ok(bytes_written) => Ok(bytes_written),
            Err(e) => Err(io::Error::new(io::ErrorKind::Other, e)),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        // Memory files don't need flushing, nothing to do
        Ok(())
    }
}

// Public key for signature verification read at build time
const PUBLIC_KEY_BYTES: &[u8; 32] =
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/keys/public_key.bin"));

/// Downloads a file from the given URL directly into a MemFile
pub fn download(url: &Url) -> Result<MemFile<Verified>, DownloadError> {
    let client = create_client()?;

    // Create an empty memory file
    let mut mem_file = MemFile::create()?;

    // Send request and stream the response directly to the memory file
    let mut response = client.get(url.clone()).send()?;

    // Copy the response body directly to the file
    let bytes_copied = io::copy(&mut response, &mut mem_file)?;

    info!("Downloaded {} bytes directly to memory file", bytes_copied);

    // Verify signature and truncate the file
    mem_file.verify_signature().map_err(DownloadError::MemFile)
}

/// Creates an HTTP client with security settings similar to the update-agent
fn create_client() -> Result<Client, DownloadError> {
    // Compile-time assertion to ensure allow_http feature isn't enabled in release mode
    #[cfg(all(feature = "allow_http", not(debug_assertions)))]
    compile_error!("The 'allow_http' feature cannot be enabled in release mode for security reasons");

    let builder = Client::builder()
        .tls_built_in_root_certs(true)
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION")
        ))
        .timeout(Duration::from_secs(120));

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
            .min_tls_version(reqwest::tls::Version::TLS_1_3)
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
