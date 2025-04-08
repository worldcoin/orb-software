use std::{
    ffi::CString,
    fs::File,
    io::{self, Seek, SeekFrom, Write},
    ops::Deref,
    os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
    path::Path,
    time::Duration,
    env,
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

#[derive(Debug, thiserror::Error)]
pub enum MemFileError {
    #[error("memfd_create failed: {0}")]
    MemfdCreate(#[from] Errno),
    
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    
    #[error("mmap failed: {0}")]
    Mmap(#[from] nix::Error),
    
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
    /// Create a new empty memory file with close-on-exec flag
    pub fn create() -> Result<Self, MemFileError> {
        // Create the memfd with a generic name and close-on-exec flag
        let c_name = CString::new("memfile").expect("Static string should never fail");
        let raw_fd = memfd_create(&c_name, MemFdCreateFlag::MFD_CLOEXEC)? as RawFd;
        let fd = unsafe { OwnedFd::from_raw_fd(raw_fd) };
        
        Ok(Self { fd })
    }
    
    /// Get a reference to the memory file's file descriptor
    fn fd(&self) -> &OwnedFd {
        &self.fd
    }
    
    /// Get the raw file descriptor as RawFd
    pub fn as_raw_fd(&self) -> RawFd {
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
    
    /// Execute the memory file using fexecve
    /// 
    /// This will replace the current process with the executed program.
    /// The file descriptor must point to an executable file.
    /// 
    /// # Arguments
    /// 
    /// * `args` - Command line arguments for the executable
    /// * `envp` - Environment variables for the executable (if None, uses current environment)
    /// 
    /// # Returns
    /// 
    /// This function only returns on error. On success, the current process is replaced.
    pub fn execute<S: AsRef<str>>(
        &self,
        args: &[S],
        envp: Option<&[S]>,
    ) -> Result<!, ExecuteError> {
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
            let c_arg = CString::new(arg.as_ref()).map_err(|_| ExecuteError::Environment)?;
            c_args.push(c_arg);
        }
        
        // Prepare environment variables
        let c_env: Vec<CString> = match envp {
            Some(envp) => {
                // Use provided environment
                let mut env_vec = Vec::with_capacity(envp.len());
                for var in envp {
                    let c_var = CString::new(var.as_ref()).map_err(|_| ExecuteError::Environment)?;
                    env_vec.push(c_var);
                }
                env_vec
            },
            None => {
                // Use current environment
                env::vars()
                    .map(|(key, value)| {
                        CString::new(format!("{}={}", key, value))
                            .map_err(|_| ExecuteError::Environment)
                    })
                    .collect::<Result<Vec<CString>, ExecuteError>>()?
            }
        };
        
        // Prepare pointers for fexecve
        let mut arg_ptrs: Vec<*const libc::c_char> = c_args.iter()
            .map(|arg| arg.as_ptr())
            .collect();
        arg_ptrs.push(std::ptr::null()); // NULL-terminate the arguments
        
        let mut env_ptrs: Vec<*const libc::c_char> = c_env.iter()
            .map(|env| env.as_ptr())
            .collect();
        env_ptrs.push(std::ptr::null()); // NULL-terminate the environment
        
        // Execute the file
        info!("Executing memory file with fd: {}", self.as_raw_fd());
        let ret = unsafe {
            libc::fexecve(
                self.as_raw_fd(),
                arg_ptrs.as_ptr() as *mut *mut libc::c_char,
                env_ptrs.as_ptr() as *mut *mut libc::c_char,
            )
        };
        
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
        .user_agent(concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION")))
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
/// * `envp` - Environment variables for the executable (if None, uses current environment)
/// 
/// # Returns
/// 
/// This function only returns on error. On success, the current process is replaced.
pub fn download_and_execute<S: AsRef<str>>(
    url: &Url,
    args: &[S],
    envp: Option<&[S]>,
) -> Result<!, DownloadError> {
    let mem_file = download(url)?;
    
    // Execute the downloaded file
    mem_file.execute(args, envp)
        .map_err(|e| match e {
            ExecuteError::MemFile(e) => DownloadError::MemFile(e),
            ExecuteError::Io(e) => DownloadError::Io(e),
            ExecuteError::PermissionDenied => DownloadError::Io(
                io::Error::new(io::ErrorKind::PermissionDenied, "Permission denied")
            ),
            ExecuteError::Environment => DownloadError::Io(
                io::Error::new(io::ErrorKind::Other, "Failed to prepare execution environment")
            ),
        })
}
