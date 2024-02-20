//! Efivar module to read and write efivar files.
//! Submodules represent data for worldcoin fork of the Nvidia's EDK II implementation.
//!
//! [Nvidia Source](https://github.com/NVIDIA/edk2-nvidia)
//!
//! [Worldcoin Fork](https://github.com/worldcoin/edk2-nvidia)
//!
//! [efivar Documentation](https://www.kernel.org/doc/html/latest/filesystems/efivarfs.html)

use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub mod bootchain;
pub mod rootfs;

use crate::ioctl;
use crate::Error;

// Slots.
pub const SLOT_A: u8 = 0;
pub const SLOT_B: u8 = 1;

/// Rootfs status.
pub const ROOTFS_STATUS_NORMAL: u8 = 0;
pub const ROOTFS_STATUS_UPD_IN_PROCESS: u8 = 1;
pub const ROOTFS_STATUS_UPD_DONE: u8 = 2;
pub const ROOTFS_STATUS_UNBOOTABLE: u8 = 3;

const EFIVARS_PATH: &str = "/sys/firmware/efi/efivars/";
/// Efivar representation.
pub struct EfiVar {
    // Path to efivar.
    path: PathBuf,
}

impl EfiVar {
    /// Construct a new `EfiVar` from a `path`. If the path is relative, it is relative to the efivarfs.
    ///
    /// # Panics
    ///
    /// If the `path` is an absolute path outside of the efivarfs.
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, Error> {
        let path = PathBuf::from(EFIVARS_PATH).join(path);
        assert!(
            path.starts_with(EFIVARS_PATH),
            "Absolute path must be inside efivarfs"
        );

        Ok(EfiVar { path })
    }

    /// Read the efivar data from a `path`.
    ///
    /// Errors: i/o specific on file operations and `InvalidEfiVarLen` if the data length is invalid.
    pub fn read(&self) -> Result<Vec<u8>, Error> {
        let mut file =
            File::open(&self.path).map_err(|e| Error::open_file(&self.path, e))?;
        let mut buffer: Vec<u8> = vec![];
        file.read_to_end(&mut buffer)
            .map_err(|e| Error::read_file(&self.path, e))?;
        Ok(buffer)
    }

    /// Read the efivar data from a `path`.
    /// Validates the expected data length and saves the data to a `buffer`.
    ///
    pub fn read_fixed_len(&self, expected_length: usize) -> Result<Vec<u8>, Error> {
        let buf = self.read()?;
        is_valid_buffer(&buf, expected_length)?;
        Ok(buf)
    }
    /// Validates the expected data length and writes the current buffer.
    ///
    /// Errors: i/o specific `Error`s on file operations and `InvalidEfiVarLen` if the data length is invalid.
    pub fn write(self, buffer: &[u8]) -> Result<(), Error> {
        let file_read =
            File::open(&self.path).map_err(|e| Error::open_file(&self.path, e))?;

        let original_attributes: libc::c_int =
            ioctl::read_file_attributes(&file_read).map_err(Error::GetAttributes)?;

        // Make file mutable.
        let new_attributes = original_attributes & !ioctl::IMMUTABLE_MASK;
        ioctl::write_file_attributes(&file_read, new_attributes)
            .map_err(Error::MakeMutable)?;

        // Open file for writing and write buffer.
        let file_write = File::options()
            .write(true)
            .open(&self.path)
            .map_err(|e| Error::open_write_file(&self.path, e))?;
        (&file_write)
            .write_all(buffer)
            .map_err(|e| Error::write_file(&self.path, e))?;
        (&file_write)
            .flush()
            .map_err(|e| Error::flush_file(&self.path, e))?;

        // Make file immutable again.
        ioctl::write_file_attributes(&file_read, original_attributes)
            .map_err(Error::MakeImmutable)?;

        Ok(())
    }

    /// Create a new efivar and write the `buffer`.
    ///
    /// Errors: i/o specific `Error`s on file operations and `InvalidEfiVarLen` if the data length is invalid.
    pub fn create_and_write(&self, buffer: &[u8]) -> Result<(), Error> {
        let inner_file =
            File::create(&self.path).map_err(|e| Error::create_file(&self.path, e))?;
        (&inner_file)
            .write_all(buffer)
            .map_err(|e| Error::write_file(&self.path, e))?;
        (&inner_file)
            .flush()
            .map_err(|e| Error::flush_file(&self.path, e))?;
        Ok(())
    }

    /// Remove UEFI variable
    pub fn remove(&self) -> Result<(), Error> {
        std::fs::remove_file(&self.path)
            .map_err(|e| Error::remove_efi_var(&self.path, e))
    }
}

/// Throws an `Error` if the given buffer is invalid.
fn is_valid_buffer(buffer: &[u8], expected_length: usize) -> Result<(), Error> {
    if buffer.len() != expected_length {
        return Err(Error::InvalidEfiVarLen);
    }
    Ok(())
}
