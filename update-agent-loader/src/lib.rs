// We're not using the never type directly since it's not stable yet

use std::{
    ffi::CString,
    io::{self, Write},
    num::NonZeroUsize,
    ops::Deref,
    os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
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
    ZeroSize,
}

impl std::fmt::Display for MemFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MemfdCreate(e) => write!(f, "memfd_create failed: {}", e),
            Self::Io(e) => write!(f, "I/O error: {}", e),
            Self::Mmap(e) => write!(f, "mmap failed: {}", e),
            Self::Fstat(e) => write!(f, "fstat failed: {}", e),
            Self::ZeroSize => write!(f, "zero-sized file"),
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

        // Convert size to NonZeroUsize for mmap
        let non_zero_size =
            NonZeroUsize::new(size as usize).ok_or(MemFileError::ZeroSize)?;

        // Create a read-only memory mapping of the file
        let mapped_ptr = unsafe {
            let result = mmap(
                None,
                non_zero_size,
                ProtFlags::PROT_READ,
                MapFlags::MAP_SHARED,
                file.as_raw_fd(),
                0,
            );

            match result {
                Ok(ptr) => ptr,
                Err(e) => return Err(MemFileError::Mmap(e)),
            }
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

impl MemFile {
    /// Create a new empty memory file with close-on-exec flag
    pub fn create() -> Result<Self, MemFileError> {
        // Create the memfd with a generic name and close-on-exec flag
        let c_name = CString::new("memfile").expect("Static string should never fail");
        let raw_fd = memfd_create(&c_name, MemFdCreateFlag::MFD_CLOEXEC)? as RawFd;
        let fd = unsafe { OwnedFd::from_raw_fd(raw_fd) };

        Ok(Self { fd })
    }

    // Removed the unused fd method

    /// Get the raw file descriptor as RawFd
    pub fn as_raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }

    /// Get the size of the memory file
    fn size(&self) -> Result<u64, MemFileError> {
        match fstat(self.fd.as_raw_fd()) {
            Ok(stat) => Ok(stat.st_size as u64),
            Err(e) => Err(MemFileError::Fstat(e)),
        }
    }

    /// Create a memory-mapped view of the file
    fn mmap(&self) -> Result<MemFileMMap<'_>, MemFileError> {
        MemFileMMap::new(self)
    }

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
    pub fn execute<S: AsRef<str>>(&self, args: &[S]) -> Result<(), ExecuteError> {
        // Verify that the file is not empty
        if self.size()? == 0 {
            return Err(ExecuteError::MemFile(MemFileError::ZeroSize));
        }

        // Prepare arguments for fexecve
        let mut c_args: Vec<CString> = Vec::with_capacity(args.len() + 1);

        // Add the program name as first argument (args[0])
        c_args.push(CString::new("memfile").map_err(|_| ExecuteError::Environment)?);

        // Add the rest of the arguments
        for arg in args {
            let c_arg =
                CString::new(arg.as_ref()).map_err(|_| ExecuteError::Environment)?;
            c_args.push(c_arg);
        }

        // Prepare pointers for fexecve
        let mut arg_ptrs: Vec<*const libc::c_char> =
            c_args.iter().map(|arg| arg.as_ptr()).collect();
        arg_ptrs.push(std::ptr::null()); // NULL-terminate the arguments

        // Get the current environment
        extern "C" {
            static environ: *const *const libc::c_char;
        }

        // Execute the file
        info!("Executing memory file with fd: {}", self.as_raw_fd());
        let ret =
            unsafe { libc::fexecve(self.as_raw_fd(), arg_ptrs.as_ptr(), environ) };

        // If we get here, fexecve failed
        if ret == -1 {
            let err = io::Error::last_os_error();
            match err.kind() {
                io::ErrorKind::PermissionDenied => Err(ExecuteError::PermissionDenied),
                _ => Err(ExecuteError::Io(err)),
            }
        } else {
            // This should never happen, as fexecve returns -1 on error
            // and doesn't return on success
            Err(ExecuteError::Io(io::Error::new(
                io::ErrorKind::Other,
                "fexecve returned an unexpected value",
            )))
        }
    }
}

impl Write for MemFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let bytes_written = unsafe {
            libc::write(
                self.fd.as_raw_fd(),
                buf.as_ptr() as *const libc::c_void,
                buf.len(),
            )
        };

        if bytes_written < 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(bytes_written as usize)
    }

    fn flush(&mut self) -> io::Result<()> {
        // Memory files don't need flushing, nothing to do
        Ok(())
    }
}

/// Downloads a file from the given URL directly into a MemFile
pub fn download(url: &Url) -> Result<MemFile, DownloadError> {
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
        .user_agent(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION")
        ))
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(DownloadError::ClientError)
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
pub fn download_and_execute<S: AsRef<str>>(
    url: &Url,
    args: &[S],
) -> Result<(), DownloadError> {
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
