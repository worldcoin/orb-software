//! Module for in-memory file backed by memfd and its memory-mapped view

use std::{
    env,
    ffi::{c_void, CString},
    io::{self, Write},
    marker::PhantomData,
    num::NonZeroUsize,
    ops::Deref,
    os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
};

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use nix::{
    errno::Errno,
    fcntl::{fcntl, FcntlArg, SealFlag},
    sys::{
        memfd::{memfd_create, MemFdCreateFlag},
        mman::{mmap, munmap, MapFlags, ProtFlags},
        stat::{fchmod, fstat, Mode},
    },
    unistd::{fexecve, ftruncate, write},
};

#[derive(Debug)]
pub enum MemFileError {
    MemfdCreate(Errno),
    Io(io::Error),
    Mmap(nix::Error),
    Fstat(nix::Error),
    /// Failed to parse or verify signature
    SignatureError(String),
    /// Failed to apply seals on the memory file
    SealError(Errno),
    /// Failed to set execute permissions
    ChmodError(nix::Error),
}

impl std::fmt::Display for MemFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MemfdCreate(e) => write!(f, "memfd_create failed: {e}"),
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Mmap(e) => write!(f, "mmap failed: {e}"),
            Self::Fstat(e) => write!(f, "fstat failed: {e}"),
            Self::SignatureError(msg) => write!(f, "signature error: {msg}"),
            Self::SealError(e) => write!(f, "failed to apply seals: {e}"),
            Self::ChmodError(e) => {
                write!(f, "failed to set execute permissions: {e}")
            }
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
    marker: PhantomData<S>,
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
        // Create a memfd with close-on-exec and allow sealing
        let raw_fd = memfd_create(
            &c_name,
            MemFdCreateFlag::MFD_CLOEXEC | MemFdCreateFlag::MFD_ALLOW_SEALING,
        )? as RawFd;
        let fd = unsafe { OwnedFd::from_raw_fd(raw_fd) };

        Ok(Self {
            fd,
            marker: PhantomData,
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
            .map_err(|e| MemFileError::Io(io::Error::other(e)))?;

        Ok(())
    }
    /// Verifies the signature in the file
    ///
    /// This reads the magic bytes "$WLD TO THE MOON" at the end of the file
    /// and the 4 bytes before that determine the signature size.
    /// It extracts and verifies the signature, then truncates the file
    /// to remove the signature.
    pub fn verify_signature(
        self,
        pubkey: VerifyingKey,
    ) -> Result<MemFile<Verified>, MemFileError> {
        // Create a memory-mapped view of the file
        let mmap = self.mmap()?;

        // Define magic bytes
        const MAGIC_BYTES: &[u8] = b"$WLD TO THE MOON";
        const MAGIC_SIZE: usize = MAGIC_BYTES.len();
        const SIG_SIZE_BYTES: usize = 4;
        const FOOTER_SIZE: usize = MAGIC_SIZE + SIG_SIZE_BYTES;

        // Check if the file is large enough to contain at least the magic bytes and signature size
        if mmap.len() < FOOTER_SIZE {
            return Err(MemFileError::Io(io::Error::new(
                io::ErrorKind::InvalidData,
                "File too small to contain magic bytes and signature size",
            )));
        }

        // Check for magic bytes at the end of the file
        let file_magic_bytes = &mmap[mmap.len() - MAGIC_SIZE..];
        if file_magic_bytes != MAGIC_BYTES {
            return Err(MemFileError::Io(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "Invalid file format: missing magic bytes (expected {MAGIC_BYTES:?})",
                ),
            )));
        }

        // Read the signature size from the 4 bytes before the magic bytes (u32, little-endian)
        let sig_size_bytes = &mmap[mmap.len() - FOOTER_SIZE..mmap.len() - MAGIC_SIZE];
        let sig_size = u32::from_le_bytes([
            sig_size_bytes[0],
            sig_size_bytes[1],
            sig_size_bytes[2],
            sig_size_bytes[3],
        ]) as usize;

        // Check if file is large enough to contain the signature + footer
        if mmap.len() < sig_size + FOOTER_SIZE {
            return Err(MemFileError::Io(io::Error::new(
                io::ErrorKind::InvalidData,
                "File too small to contain signature",
            )));
        }

        // Extract the signature
        let sig_data =
            &mmap[mmap.len() - sig_size - FOOTER_SIZE..mmap.len() - FOOTER_SIZE];

        // Copy signature to a fixed-size buffer for ed25519-dalek
        let mut sig_bytes = [0u8; 64];
        // Ensure signature is exactly 64 bytes
        if sig_size != sig_bytes.len() {
            return Err(MemFileError::SignatureError(format!(
                "Unexpected signature length: {} bytes, expected {} bytes",
                sig_size,
                sig_bytes.len()
            )));
        }
        sig_bytes.copy_from_slice(sig_data);
        // Create the signature object (panics if invalid)
        let signature = Signature::from_bytes(&sig_bytes);

        // The data to verify is everything except the signature, its size and magic bytes
        let data = &mmap[..mmap.len() - sig_size - FOOTER_SIZE];

        // Truncate the file to remove the signature, size field and magic bytes
        let new_size = self.size()? - (sig_size as u64) - (FOOTER_SIZE as u64);
        self.truncate(new_size)?;

        // Verify the signature
        pubkey.verify(data, &signature).map_err(|e| {
            MemFileError::SignatureError(format!(
                "Signature verification failed: {}, used pubkey {:x?}",
                e,
                pubkey.as_bytes()
            ))
        })?;

        drop(mmap);
        // Make the in-memory file executable
        fchmod(self.fd.as_raw_fd(), Mode::from_bits_truncate(0o500))
            .map_err(MemFileError::ChmodError)?;
        // Seal the file against further modifications
        let seals = SealFlag::F_SEAL_SHRINK
            | SealFlag::F_SEAL_GROW
            | SealFlag::F_SEAL_WRITE
            | SealFlag::F_SEAL_SEAL;
        fcntl(self.fd.as_raw_fd(), FcntlArg::F_ADD_SEALS(seals))
            .map_err(MemFileError::SealError)?;
        // Return the verified, sealed, executable file
        Ok(MemFile {
            fd: self.fd,
            marker: PhantomData,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ExecuteError {
    #[error("Failed to execute: {0}")]
    Io(#[from] io::Error),

    #[error("Permission denied")]
    PermissionDenied,

    #[error("MemFd error: {0}")]
    MemFile(#[from] MemFileError),

    #[error("Failed to prepare execution environment")]
    Environment,
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
            let c_arg =
                CString::new(arg as &str).map_err(|_| ExecuteError::Environment)?;
            c_args.push(c_arg);
        }

        // We don't need pointers for nix::unistd::fexecve

        // Convert the args to a slice of CStr references as required by nix::unistd::fexecve
        let args_cstr: Vec<&std::ffi::CStr> =
            c_args.iter().map(|arg| arg.as_c_str()).collect();

        // Get current environment variables and convert to CString
        let env_vars: Vec<CString> = env::vars()
            .map(|(key, val)| CString::new(format!("{key}={val}")))
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
                Err(ExecuteError::Io(io::Error::other(
                    "fexecve returned unexpectedly",
                )))
            }
            Err(nix::Error::EPERM) => {
                // Permission denied
                Err(ExecuteError::PermissionDenied)
            }
            Err(e) => {
                // Convert other nix errors to io::Error
                Err(ExecuteError::Io(io::Error::other(format!(
                    "fexecve failed: {e}",
                ))))
            }
        }
    }
}

impl Write for MemFile<Unverified> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match write(self.fd.as_raw_fd(), buf) {
            Ok(bytes_written) => Ok(bytes_written),
            Err(e) => Err(io::Error::other(e)),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        // Memory files don't need flushing, nothing to do
        Ok(())
    }
}
