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

/// Efivar representation.
pub struct EfiVar {
    // Path to efivar.
    path: &'static str,
    // Open readonly `File`.
    inner_file: File,
    // Expected length of efivar.
    expected_length: usize,
    // Original file attributes.
    original_attributes: libc::c_int,
    /// Data buffer.
    buffer: Vec<u8>,
}

impl EfiVar {
    /// Read the efivar data from a `path`.
    /// Validates the expected data length and saves the data to a `buffer`.
    ///
    /// Errors: i/o specific on file operations and `InvalidEfiVarLen` if the data length is invalid.
    pub fn open(path: &'static str, expected_length: usize) -> Result<Self, Error> {
        let inner_file = File::open(path).map_err(|e| Error::open_file(path, e))?;
        let original_attributes: libc::c_int =
            ioctl::read_file_attributes(&inner_file).map_err(Error::GetAttributes)?;
        let mut buffer: Vec<u8> = vec![];
        (&inner_file)
            .read_to_end(&mut buffer)
            .map_err(|e| Error::read_file(path, e))?;
        is_valid_buffer(&buffer, expected_length)?;
        Ok(Self {
            path,
            inner_file,
            expected_length,
            original_attributes,
            buffer,
        })
    }

    /// Validates the expected data length and writes the current buffer.
    ///
    /// Errors: i/o specific `Error`s on file operations and `InvalidEfiVarLen` if the data length is invalid.
    pub fn write(self) -> Result<(), Error> {
        is_valid_buffer(&self.buffer, self.expected_length)?;

        // Make file mutable.
        let new_attributes = self.original_attributes & !ioctl::IMMUTABLE_MASK;
        ioctl::write_file_attributes(&self.inner_file, new_attributes)
            .map_err(Error::MakeMutable)?;

        // Open file for writing and write buffer.
        let file_write = File::options()
            .write(true)
            .open(self.path)
            .map_err(|e| Error::open_write_file(self.path, e))?;
        (&file_write)
            .write_all(&self.buffer)
            .map_err(|e| Error::write_file(self.path, e))?;
        (&file_write)
            .flush()
            .map_err(|e| Error::flush_file(self.path, e))?;

        // Make file immutable again.
        ioctl::write_file_attributes(&self.inner_file, self.original_attributes)
            .map_err(Error::MakeImmutable)?;

        Ok(())
    }

    /// Create a new efivar and write the `buffer`.
    ///
    /// Errors: i/o specific `Error`s on file operations and `InvalidEfiVarLen` if the data length is invalid.
    pub fn create_and_write(
        path: &'static str,
        buffer: Vec<u8>,
        expected_length: usize,
    ) -> Result<Self, Error> {
        is_valid_buffer(&buffer, expected_length)?;

        let inner_file = File::create(path).map_err(|e| Error::create_file(path, e))?;
        (&inner_file)
            .write_all(&buffer)
            .map_err(|e| Error::write_file(path, e))?;
        (&inner_file)
            .flush()
            .map_err(|e| Error::flush_file(path, e))?;

        Ok(Self {
            path,
            inner_file,
            expected_length,
            original_attributes: 0,
            buffer,
        })
    }
}

/// Throws an `Error` if the given buffer is invalid.
fn is_valid_buffer(buffer: &[u8], expected_length: usize) -> Result<(), Error> {
    if buffer.len() != expected_length {
        return Err(Error::InvalidEfiVarLen);
    }
    Ok(())
}
