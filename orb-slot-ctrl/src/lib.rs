//! The rust program for reading and writing the slot and rootfs state of the Orb.

#![allow(clippy::missing_errors_doc)]

use std::{
    fmt, io,
    path::{Path, PathBuf},
};

mod efivar;
mod ioctl;
pub mod program;

pub mod test_utils;

use efivar::{
    bootchain::BootChainEfiVars, rootfs::RootfsEfiVars, EfiVarDbErr,
    ROOTFS_STATUS_NORMAL, ROOTFS_STATUS_UNBOOTABLE, ROOTFS_STATUS_UPD_DONE,
    ROOTFS_STATUS_UPD_IN_PROCESS, SLOT_A, SLOT_B,
};

pub use crate::efivar::{EfiVar, EfiVarDb};

/// Error definition for library.
#[allow(missing_docs)]
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("failed getting attributes using FS_ICT_GETFLAGS ioctl command: {0}")]
    GetAttributes(io::Error),
    #[error(
        "failed unsetting immutable flag using FS_ICT_SETFLAGS ioctl command: {0}"
    )]
    MakeMutable(io::Error),
    #[error("failed setting immutable flag using FS_ICT_SETFLAGS ioctl command: {0}")]
    MakeImmutable(io::Error),
    #[error("failed opening file {path} for reading: {source}")]
    OpenFile { path: PathBuf, source: io::Error },
    #[error("failed opening file {path} for writing: {source}")]
    OpenWriteFile { path: PathBuf, source: io::Error },
    #[error("failed opening file {path} for reading: {source}")]
    CreateFile { path: PathBuf, source: io::Error },
    #[error("failed reading file to buffer: {source}")]
    ReadFile { path: PathBuf, source: io::Error },
    #[error("failed writing file from buffer: {source}")]
    WriteFile { path: PathBuf, source: io::Error },
    #[error("failed flushing file {path}: {source}")]
    FlushFile { path: PathBuf, source: io::Error },
    #[error("failed to remove EFI variable {path}: {source}")]
    RemoveEfiVar { path: PathBuf, source: io::Error },
    #[error("failed reading efivar, invalid data length")]
    InvalidEfiVarLen,
    #[error("invalid slot configuration")]
    InvalidSlotData,
    #[error("invalid rootfs status")]
    InvalidRootFsStatusData,
    #[error("invalid retry counter({counter}), exceeding the maximum ({max})")]
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
    pub fn remove_efi_var<P: AsRef<Path>>(path: P, source: io::Error) -> Self {
        Self::RemoveEfiVar {
            path: path.as_ref().to_path_buf(),
            source,
        }
    }
}

/// Representation of the slot.
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(u8)]
pub enum Slot {
    /// The Slot A is represented as 0.
    A = SLOT_A,
    /// The Slot B is represented as 1.
    B = SLOT_B,
}

/// Format slot as lowercase to match Nvidia standard in file system.
impl fmt::Display for Slot {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Slot::A => write!(f, "a"),
            Slot::B => write!(f, "b"),
        }
    }
}

/// Representation of the rootfs status.
#[derive(Debug, PartialEq)]
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

pub struct OrbSlotCtrl {
    bootchain: BootChainEfiVars,
    rootfs: RootfsEfiVars,
}

impl OrbSlotCtrl {
    pub fn new(db: &EfiVarDb) -> Result<Self, EfiVarDbErr> {
        Ok(Self {
            bootchain: BootChainEfiVars::new(db)?,
            rootfs: RootfsEfiVars::new(db)?,
        })
    }

    /// Get the current active slot.
    pub fn get_current_slot(&self) -> Result<Slot, Error> {
        match self.bootchain.get_current_boot_slot()? {
            SLOT_A => Ok(Slot::A),
            SLOT_B => Ok(Slot::B),
            _ => Err(Error::InvalidSlotData),
        }
    }

    /// Get the inactive slot.
    pub fn get_inactive_slot(&self) -> Result<Slot, Error> {
        // inverts the output of `get_current_slot()`
        match self.get_current_slot()? {
            Slot::A => Ok(Slot::B),
            Slot::B => Ok(Slot::A),
        }
    }

    /// Get the slot set for the next boot.
    pub fn get_next_boot_slot(&self) -> Result<Slot, Error> {
        match self.bootchain.get_next_boot_slot()? {
            SLOT_A => Ok(Slot::A),
            SLOT_B => Ok(Slot::B),
            _ => Err(Error::InvalidSlotData),
        }
    }

    /// Set the slot for the next boot.
    pub fn set_next_boot_slot(&self, slot: Slot) -> Result<(), Error> {
        self.reset_retry_count_to_max(slot)?;
        self.bootchain.set_next_boot_slot(slot as u8)
    }

    /// Get the rootfs status for the current active slot.
    pub fn get_current_rootfs_status(&self) -> Result<RootFsStatus, Error> {
        RootFsStatus::try_from(
            self.rootfs
                .get_rootfs_status(self.bootchain.get_current_boot_slot()?)?,
        )
    }

    /// Get the rootfs status for a certain `slot`.
    pub fn get_rootfs_status(&self, slot: Slot) -> Result<RootFsStatus, Error> {
        RootFsStatus::try_from(self.rootfs.get_rootfs_status(slot as u8)?)
    }

    /// Set a rootfs status for the current active slot.
    pub fn set_current_rootfs_status(&self, status: RootFsStatus) -> Result<(), Error> {
        self.rootfs
            .set_rootfs_status(status as u8, self.bootchain.get_current_boot_slot()?)
    }

    /// Set a rootfs status for a certain `slot`.
    pub fn set_rootfs_status(
        &self,
        status: RootFsStatus,
        slot: Slot,
    ) -> Result<(), Error> {
        self.rootfs.set_rootfs_status(status as u8, slot as u8)
    }

    /// Get the retry count for the current active slot.
    pub fn get_current_retry_count(&self) -> Result<u8, Error> {
        self.rootfs
            .get_retry_count(self.bootchain.get_current_boot_slot()?)
    }

    /// Get the retry count for a certain `slot`.
    pub fn get_retry_count(&self, slot: Slot) -> Result<u8, Error> {
        self.rootfs.get_retry_count(slot as u8)
    }

    /// Get the maximum retry count before fallback.
    pub fn get_max_retry_count(&self) -> Result<u8, Error> {
        self.rootfs.get_max_retry_count()
    }

    /// Reset the retry counter to the maximum for the current active slot.
    pub fn reset_current_retry_count_to_max(&self) -> Result<(), Error> {
        let max_count = self.rootfs.get_max_retry_count()?;
        self.rootfs
            .set_retry_count(max_count, self.bootchain.get_current_boot_slot()?)
    }

    /// Reset the retry counter to the maximum for the a certain `slot`.
    pub fn reset_retry_count_to_max(&self, slot: Slot) -> Result<(), Error> {
        let max_count = self.rootfs.get_max_retry_count()?;
        self.rootfs.set_retry_count(max_count, slot as u8)
    }
}
