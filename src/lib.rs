//! The rust program for reading and writing the slot and rootfs state of the Orb.
//!
//! # Guidelines
//!
//! The code should be formatted with Rustfmt.
//! I.e. run from the command line: `cargo fmt`.
//!
//! The code should pass clippy lints in pedantic mode.
//! I.e. run from the command line: `cargo clippy`.
//! It's fine to suppress some lint locally with `#[allow(clippy:<lint>)]` attribute.
#![warn(clippy::pedantic, missing_docs)]
#![allow(clippy::missing_errors_doc)]

use std::{
    io,
    path::{Path, PathBuf},
};

mod efivar;
mod ioctl;

use efivar::{
    ROOTFS_STATUS_NORMAL, ROOTFS_STATUS_UNBOOTABLE, ROOTFS_STATUS_UPD_DONE,
    ROOTFS_STATUS_UPD_IN_PROCESS, SLOT_A, SLOT_B,
};

/// Error definition for library.
#[allow(missing_docs)]
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("failed getting attributes using FS_ICT_GETFLAGS ioctl command")]
    GetAttributes(io::Error),
    #[error("failed unsetting immutable flag using FS_ICT_SETFLAGS ioctl command")]
    MakeMutable(io::Error),
    #[error("failed setting immutable flag using FS_ICT_SETFLAGS ioctl command")]
    MakeImmutable(io::Error),
    #[error("failed opening file for reading")]
    OpenFile { path: PathBuf, source: io::Error },
    #[error("failed opening file for writing")]
    OpenWriteFile { path: PathBuf, source: io::Error },
    #[error("failed opening file for reading")]
    CreateFile { path: PathBuf, source: io::Error },
    #[error("failed reading file to buffer")]
    ReadFile { path: PathBuf, source: io::Error },
    #[error("failed writing file from buffer")]
    WriteFile { path: PathBuf, source: io::Error },
    #[error("failed flushing file")]
    FlushFile { path: PathBuf, source: io::Error },
    #[error("failed reading efivar, invalid data length")]
    InvalidEfiVarLen,
    #[error("invalid slot configuration")]
    InvalidSlotData,
    #[error("invalid rootfs status")]
    InvalidRootFsStatusData,
    #[error("invalid retry counter, exceeding the maximum")]
    ExceedingRetryCount { counter: u8, max: u8 },
}

#[allow(missing_docs)]
impl Error {
    pub fn open_file<P: AsRef<Path>>(path: P, source: io::Error) -> Self {
        Self::OpenFile {
            path: path.as_ref().to_path_buf(),
            source,
        }
    }
    pub fn open_write_file<P: AsRef<Path>>(path: P, source: io::Error) -> Self {
        Self::OpenWriteFile {
            path: path.as_ref().to_path_buf(),
            source,
        }
    }
    pub fn create_file<P: AsRef<Path>>(path: P, source: io::Error) -> Self {
        Self::CreateFile {
            path: path.as_ref().to_path_buf(),
            source,
        }
    }
    pub fn read_file<P: AsRef<Path>>(path: P, source: io::Error) -> Self {
        Self::ReadFile {
            path: path.as_ref().to_path_buf(),
            source,
        }
    }
    pub fn write_file<P: AsRef<Path>>(path: P, source: io::Error) -> Self {
        Self::WriteFile {
            path: path.as_ref().to_path_buf(),
            source,
        }
    }
    pub fn flush_file<P: AsRef<Path>>(path: P, source: io::Error) -> Self {
        Self::FlushFile {
            path: path.as_ref().to_path_buf(),
            source,
        }
    }
}

/// Representation of the slot.
#[derive(Debug)]
#[repr(u8)]
pub enum Slot {
    /// The Slot A is represented as 0.
    A = SLOT_A,
    /// The Slot B is represented as 1.
    B = SLOT_B,
}

/// Representation of the rootfs status.
#[derive(Debug)]
#[repr(u8)]
pub enum RootFsStatus {
    /// Default status of the rootfs.
    Normal = ROOTFS_STATUS_NORMAL,
    /// Status of the rootfs where the partitions during an update are written.
    UpdateInProcess = ROOTFS_STATUS_UPD_IN_PROCESS,
    /// Status of the rootfs where the boot slot was just switched to it.
    UpdateDone = ROOTFS_STATUS_UPD_DONE,
    /// Status of the rootfs is considered unbootable.
    Unbootable = ROOTFS_STATUS_UNBOOTABLE,
}

impl RootFsStatus {
    /// Checks if current status is `RootFsStats::Normal`.
    #[must_use]
    pub fn is_normal(self) -> bool {
        matches!(self, Self::Normal)
    }

    /// Checks if current status is `RootFsStats::UpdateInProcess`.
    #[must_use]
    pub fn is_update_in_progress(self) -> bool {
        matches!(self, Self::UpdateInProcess)
    }

    /// Checks if current status is `RootFsStats::UpdateDone`.
    #[must_use]
    pub fn is_update_done(self) -> bool {
        matches!(self, Self::UpdateDone)
    }

    /// Checks if current status is `RootFsStats::Unbootable`.
    #[must_use]
    pub fn is_unbootable(self) -> bool {
        matches!(self, Self::Unbootable)
    }
}

impl TryFrom<u8> for RootFsStatus {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self, Error> {
        match value {
            ROOTFS_STATUS_NORMAL => Ok(RootFsStatus::Normal),
            ROOTFS_STATUS_UPD_IN_PROCESS => Ok(RootFsStatus::UpdateInProcess),
            ROOTFS_STATUS_UPD_DONE => Ok(RootFsStatus::UpdateDone),
            ROOTFS_STATUS_UNBOOTABLE => Ok(RootFsStatus::Unbootable),
            _ => Err(Error::InvalidRootFsStatusData),
        }
    }
}

/// Get the current active slot.
pub fn get_current_slot() -> Result<Slot, Error> {
    match efivar::bootchain::get_current_boot_slot()? {
        SLOT_A => Ok(Slot::A),
        SLOT_B => Ok(Slot::B),
        _ => Err(Error::InvalidSlotData),
    }
}

/// Get the inactive slot.
pub fn get_inactive_slot() -> Result<Slot, Error> {
    // inverts the output of `get_current_slot()`
    match get_current_slot()? {
        Slot::A => Ok(Slot::B),
        Slot::B => Ok(Slot::A),
    }
}

/// Get the slot set for the next boot.
pub fn get_next_boot_slot() -> Result<Slot, Error> {
    match efivar::bootchain::get_next_boot_slot()? {
        SLOT_A => Ok(Slot::A),
        SLOT_B => Ok(Slot::B),
        _ => Err(Error::InvalidSlotData),
    }
}

/// Set the slot for the next boot.
pub fn set_next_boot_slot(slot: Slot) -> Result<(), Error> {
    efivar::bootchain::set_next_boot_slot(slot as u8)
}

/// Get the rootfs status for the current active slot.
pub fn get_current_rootfs_status() -> Result<RootFsStatus, Error> {
    RootFsStatus::try_from(efivar::rootfs::get_rootfs_status(
        efivar::bootchain::get_current_boot_slot()?,
    )?)
}

/// Get the rootfs status for a certain `slot`.
pub fn get_rootfs_status(slot: Slot) -> Result<RootFsStatus, Error> {
    RootFsStatus::try_from(efivar::rootfs::get_rootfs_status(slot as u8)?)
}

/// Set a rootfs status for the current active slot.
pub fn set_current_rootfs_status(status: RootFsStatus) -> Result<(), Error> {
    efivar::rootfs::set_rootfs_status(status as u8, efivar::bootchain::get_current_boot_slot()?)
}

/// Set a rootfs status for a certain `slot`.
pub fn set_rootfs_status(status: RootFsStatus, slot: Slot) -> Result<(), Error> {
    efivar::rootfs::set_rootfs_status(status as u8, slot as u8)
}

/// Get the retry count for the current active slot.
pub fn get_current_retry_count() -> Result<u8, Error> {
    efivar::rootfs::get_retry_count(efivar::bootchain::get_current_boot_slot()?)
}

/// Get the retry count for a certain `slot`.
pub fn get_retry_count(slot: Slot) -> Result<u8, Error> {
    efivar::rootfs::get_retry_count(slot as u8)
}

/// Get the maximum retry count before fallback.
pub fn get_max_retry_count() -> Result<u8, Error> {
    efivar::rootfs::get_max_retry_count()
}

/// Reset the retry counter to the maximum for the current active slot.
pub fn reset_current_retry_count_to_max() -> Result<(), Error> {
    let max_count = efivar::rootfs::get_max_retry_count()?;
    efivar::rootfs::set_retry_count(max_count, efivar::bootchain::get_current_boot_slot()?)
}

/// Reset the retry counter to the maximum for the a certain `slot`.
pub fn reset_retry_count_to_max(slot: Slot) -> Result<(), Error> {
    let max_count = efivar::rootfs::get_max_retry_count()?;
    efivar::rootfs::set_retry_count(max_count, slot as u8)
}
