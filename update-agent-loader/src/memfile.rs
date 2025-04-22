//! Module for in-memory file backed by memfd and its memory-mapped view
use std::marker::PhantomData;
use std::{
    env,
    ffi::{c_void, CString},
    io::{self, Write},
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

use crate::ExecuteError;

/// Errors that can occur when working with MemFile
#[derive(Debug)]
pub enum MemFileError {
    MemfdCreate(Errno),
    Io(io::Error),
    Mmap(nix::Error),
    Fstat(nix::Error),
    SignatureError(String),
    SealError(Errno),
    ChmodError(nix::Error),
}

impl std::fmt::Display for MemFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MemfdCreate(e) => write!(f, "memfd_create failed: {}", e),
            Self::Io(e) => write!(f, "I/O error: {}", e),
            Self::Mmap(e) => write!(f, "mmap failed: {}", e),
            Self::Fstat(e) => write!(f, "fstat failed: {}", e),
            Self::SignatureError(msg) => write!(f, "signature error: {}", msg),
            Self::SealError(e) => write!(f, "failed to apply seals: {}", e),
            Self::ChmodError(e) => {
                write!(f, "failed to set execute permissions: {}", e)
            }
        }
    }
}

impl std::error::Error for MemFileError {}
impl From<Errno> for MemFileError {
    fn from(e: Errno) -> Self {
        MemFileError::MemfdCreate(e)
    }
}
impl From<io::Error> for MemFileError {
    fn from(e: io::Error) -> Self {
        MemFileError::Io(e)
    }
}

/// Marker trait for MemFile state
pub trait MemFileState {}
/// State before signature verification
pub enum Unverified {}
/// State after signature verification
pub enum Verified {}
impl MemFileState for Unverified {}
impl MemFileState for Verified {}

/// In-memory file created with memfd_create
pub struct MemFile<S: MemFileState> {
    fd: OwnedFd,
    _marker: PhantomData<S>,
}

/// Memory-mapped view of a MemFile
pub struct MemFileMMap<'a> {
    mapped_ptr: *mut c_void,
    size: usize,
    _file: &'a MemFile<Unverified>,
}

impl<'a> MemFileMMap<'a> {
    pub fn new(file: &'a MemFile<Unverified>) -> Result<Self, MemFileError> {
        let size = file.size()?;
        let non_zero = NonZeroUsize::new(size as usize).ok_or_else(|| {
            MemFileError::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Cannot map zero-sized file",
            ))
        })?;
        let ptr = unsafe {
            mmap(
                None,
                non_zero,
                ProtFlags::PROT_READ,
                MapFlags::MAP_PRIVATE,
                file.as_raw_fd(),
                0,
            )
            .map_err(MemFileError::Mmap)?
        };
        Ok(Self {
            mapped_ptr: ptr,
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
    fn size(&self) -> Result<u64, MemFileError> {
        fstat(self.fd.as_raw_fd())
            .map(|st| st.st_size as u64)
            .map_err(MemFileError::Fstat)
    }
    pub fn as_raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }
}

impl MemFile<Unverified> {
    pub fn create() -> Result<Self, MemFileError> {
        let name = CString::new("memfile").unwrap();
        let raw = memfd_create(
            &name,
            MemFdCreateFlag::MFD_CLOEXEC | MemFdCreateFlag::MFD_ALLOW_SEALING,
        )? as RawFd;
        let fd = unsafe { OwnedFd::from_raw_fd(raw) };
        Ok(Self {
            fd,
            _marker: PhantomData,
        })
    }
    fn mmap(&self) -> Result<MemFileMMap<'_>, MemFileError> {
        MemFileMMap::new(self)
    }
    fn truncate(&self, len: u64) -> Result<(), MemFileError> {
        ftruncate(self.fd.as_raw_fd(), len as i64)
            .map_err(|e| MemFileError::Io(io::Error::new(io::ErrorKind::Other, e)))?;
        Ok(())
    }
    pub fn verify_signature(self) -> Result<MemFile<Verified>, MemFileError> {
        let mmap = self.mmap()?;
        const MAGIC: &[u8] = b"$WLD TO THE MOON";
        const FOOTER: usize = MAGIC.len() + 4;
        if mmap.len() < FOOTER {
            return Err(MemFileError::Io(io::Error::new(
                io::ErrorKind::InvalidData,
                "File too small",
            )));
        }
        let end = mmap.len();
        let size_bytes = &mmap[end - FOOTER..end - MAGIC.len()];
        let sig_size = u32::from_le_bytes(size_bytes.try_into().unwrap()) as usize;
        if &mmap[end - MAGIC.len()..] != MAGIC {
            return Err(MemFileError::Io(io::Error::new(
                io::ErrorKind::InvalidData,
                "Bad magic",
            )));
        }
        if mmap.len() < sig_size + FOOTER {
            return Err(MemFileError::Io(io::Error::new(
                io::ErrorKind::InvalidData,
                "File too small for sig",
            )));
        }
        let sig_data = &mmap[end - FOOTER - sig_size..end - FOOTER];
        let mut buf = [0u8; 64];
        buf.copy_from_slice(sig_data);
        let signature = Signature::from_bytes(&buf);
        let public_key = VerifyingKey::from_bytes(PUBLIC_KEY_BYTES)
            .map_err(|e| MemFileError::SignatureError(e.to_string()))?;
        public_key
            .verify(&mmap[..end - FOOTER - sig_size], &signature)
            .map_err(|e| MemFileError::SignatureError(e.to_string()))?;
        let new_size = self.size()? - (sig_size as u64) - (FOOTER as u64);
        self.truncate(new_size)?;
        drop(mmap);
        fchmod(self.fd.as_raw_fd(), Mode::from_bits_truncate(0o500))
            .map_err(MemFileError::ChmodError)?;
        let seals = SealFlag::F_SEAL_SHRINK
            | SealFlag::F_SEAL_GROW
            | SealFlag::F_SEAL_WRITE
            | SealFlag::F_SEAL_SEAL;
        fcntl(self.fd.as_raw_fd(), FcntlArg::F_ADD_SEALS(seals))
            .map_err(MemFileError::SealError)?;
        Ok(MemFile {
            fd: self.fd,
            _marker: PhantomData,
        })
    }
}

impl MemFile<Verified> {
    pub fn execute(&self, args: &[&str]) -> Result<(), ExecuteError> {
        let sz = self.size()?;
        if sz == 0 {
            return Err(ExecuteError::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Cannot execute zero-sized file",
            )));
        }
        let mut cargs = Vec::with_capacity(args.len() + 1);
        cargs.push(CString::new("memfile").map_err(|_| ExecuteError::Environment)?);
        for &arg in args {
            cargs.push(CString::new(arg).map_err(|_| ExecuteError::Environment)?);
        }
        let arg_cstr: Vec<&std::ffi::CStr> =
            cargs.iter().map(|c| c.as_c_str()).collect();
        let envs: Vec<CString> = env::vars()
            .map(|(k, v)| CString::new(format!("{}={}", k, v)))
            .filter_map(Result::ok)
            .collect();
        let env_cstr: Vec<&std::ffi::CStr> =
            envs.iter().map(|e| e.as_c_str()).collect();
        match fexecve(self.fd.as_raw_fd(), &arg_cstr, &env_cstr) {
            Ok(_) => Err(ExecuteError::Io(io::Error::new(
                io::ErrorKind::Other,
                "fexecve returned unexpectedly",
            ))),
            Err(nix::Error::EPERM) => Err(ExecuteError::PermissionDenied),
            Err(e) => Err(ExecuteError::Io(io::Error::new(
                io::ErrorKind::Other,
                format!("fexecve failed: {}", e),
            ))),
        }
    }
}

const PUBLIC_KEY_BYTES: &[u8; 32] = include_bytes!(env!("PUBLIC_KEY_PATH"));
