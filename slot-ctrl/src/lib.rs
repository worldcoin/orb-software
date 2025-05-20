//! The rust program for reading and writing the slot and rootfs state of the Orb.

#![allow(clippy::missing_errors_doc)]

use color_eyre::eyre::Context;
use efivar::{EfiVarDb, EfiVarDbErr};
use orb_info::orb_os_release::OrbType;
use std::{fmt, path::Path};
use {bootchain::BootChainEfiVars, rootfs::RootfsEfiVars};

mod bootchain;
mod rootfs;

pub mod program;
pub mod test_utils;

// Slots.
const SLOT_A: u8 = 0;
const SLOT_B: u8 = 1;

/// Rootfs status.
const ROOTFS_STATUS_NORMAL: u8 = 0;
const ROOTFS_STATUS_UPD_IN_PROCESS: u8 = 1;
const ROOTFS_STATUS_UPD_DONE: u8 = 2;
const ROOTFS_STATUS_UNBOOTABLE: u8 = 3;

/// Error definition for library.
#[allow(missing_docs)]
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("failed reading efivar, invalid data length. expected: {expected}, actual: {actual}")]
    InvalidEfiVarLen { expected: usize, actual: usize },
    #[error("invalid slot configuration")]
    InvalidSlotData,
    #[error("invalid rootfs status")]
    InvalidRootFsStatusData,
    #[error("invalid retry counter({counter}), exceeding the maximum ({max})")]
    ExceedingRetryCount { counter: u8, max: u8 },
    #[error("{0}")]
    EfiVar(#[from] color_eyre::Report),
    #[error("{0}")]
    EfiVarDb(#[from] EfiVarDbErr),
    #[error("unsupported orb type: {0}")]
    UnsupportedOrbType(OrbType),
    #[error("{0}")]
    Verification(String),
}

type Result<T> = std::result::Result<T, Error>;

/// Representation of the slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
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

    fn try_from(value: u8) -> Result<Self> {
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
    orb_type: OrbType,
}

impl OrbSlotCtrl {
    pub fn new(rootfs: impl AsRef<Path>, orb_type: OrbType) -> Result<Self> {
        let efivardb = EfiVarDb::from_rootfs(rootfs)?;

        OrbSlotCtrl::from_evifar_db(&efivardb, orb_type)
    }

    pub fn from_evifar_db(db: &EfiVarDb, orb_type: OrbType) -> Result<Self> {
        Ok(Self {
            bootchain: BootChainEfiVars::new(db)?,
            rootfs: RootfsEfiVars::new(db)?,
            orb_type,
        })
    }

    /// Get the current active slot.
    pub fn get_current_slot(&self) -> Result<Slot> {
        match self.bootchain.get_current_boot_slot()? {
            SLOT_A => Ok(Slot::A),
            SLOT_B => Ok(Slot::B),
            _ => Err(Error::InvalidSlotData),
        }
    }

    /// Get the inactive slot.
    pub fn get_inactive_slot(&self) -> Result<Slot> {
        // inverts the output of `get_current_slot()`
        match self.get_current_slot()? {
            Slot::A => Ok(Slot::B),
            Slot::B => Ok(Slot::A),
        }
    }

    /// Get the slot set for the next boot.
    pub fn get_next_boot_slot(&self) -> Result<Slot> {
        match self.bootchain.get_next_boot_slot()? {
            SLOT_A => Ok(Slot::A),
            SLOT_B => Ok(Slot::B),
            _ => Err(Error::InvalidSlotData),
        }
    }

    /// Set the slot for the next boot.
    pub fn set_next_boot_slot(&self, slot: Slot) -> Result<()> {
        self.reset_retry_count_to_max(slot)?;
        self.bootchain.set_next_boot_slot(slot as u8)
    }

    /// Get the rootfs status for the current active slot.
    pub fn get_current_rootfs_status(&self) -> Result<RootFsStatus> {
        RootFsStatus::try_from(
            self.rootfs
                .get_rootfs_status(self.bootchain.get_current_boot_slot()?)?,
        )
    }
    /// Get the rootfs status for a certain `slot`.
    pub fn get_rootfs_status(&self, slot: Slot) -> Result<RootFsStatus> {
        RootFsStatus::try_from(self.rootfs.get_rootfs_status(slot as u8)?)
    }

    /// Set a rootfs status for the current active slot.
    pub fn set_current_rootfs_status(&self, status: RootFsStatus) -> Result<()> {
        self.rootfs
            .set_rootfs_status(status as u8, self.bootchain.get_current_boot_slot()?)
    }

    /// Set a rootfs status for a certain `slot`.
    pub fn set_rootfs_status(&self, status: RootFsStatus, slot: Slot) -> Result<()> {
        self.rootfs.set_rootfs_status(status as u8, slot as u8)
    }

    /// Get the retry count for the current active slot.
    pub(crate) fn get_current_retry_count(&self) -> Result<u8> {
        if self.orb_type == OrbType::Pearl {
            self.rootfs
                .get_retry_count(self.bootchain.get_current_boot_slot()?)
        } else {
            Err(Error::UnsupportedOrbType(self.orb_type))
        }
    }

    /// Get the retry count for a certain `slot`.
    pub(crate) fn get_retry_count(&self, slot: Slot) -> Result<u8> {
        if self.orb_type == OrbType::Pearl {
            self.rootfs.get_retry_count(slot as u8)
        } else {
            Err(Error::UnsupportedOrbType(self.orb_type))
        }
    }

    /// Get the maximum retry count before fallback.
    pub(crate) fn get_max_retry_count(&self) -> Result<u8> {
        self.rootfs.get_max_retry_count()
    }

    /// Reset the retry counter to the maximum for the current active slot.
    pub(crate) fn reset_current_retry_count_to_max(&self) -> Result<()> {
        if self.orb_type == OrbType::Pearl {
            let max_count = self.rootfs.get_max_retry_count()?;
            self.rootfs
                .set_retry_count(max_count, self.bootchain.get_current_boot_slot()?)
        } else {
            Err(Error::UnsupportedOrbType(self.orb_type))
        }
    }

    /// Reset the retry counter to the maximum for the a certain `slot`.
    pub(crate) fn reset_retry_count_to_max(&self, slot: Slot) -> Result<()> {
        if self.orb_type == OrbType::Pearl {
            let max_count = self.rootfs.get_max_retry_count()?;
            self.rootfs.set_retry_count(max_count, slot as u8)
        } else {
            Err(Error::UnsupportedOrbType(self.orb_type))
        }
    }

    /// Marks the current slot as working correctly so that
    /// Nvidia slot A/B switching redundancy mechanizm knows that this boot was successful
    pub fn mark_current_slot_ok(&self) -> Result<()> {
        // Theoretically we never reach this point if the slot is not Normal
        // But on Pearl we have 2 more states: UpdateDone + UpdateInProgress
        // TODO: Remove this once these 2 extra states are removed from edk2
        self.set_current_rootfs_status(RootFsStatus::Normal)?;
        match self.orb_type {
            OrbType::Pearl => self.reset_current_retry_count_to_max(),
            OrbType::Diamond => std::process::Command::new("nvbootctrl")
                .arg("verify")
                .output()
                .map_err(|e| Error::Verification(e.to_string()))
                .and_then(|output| match output.status.success() {
                    true => Ok(()),
                    false => Err(Error::Verification(
                        String::from_utf8_lossy(&output.stdout).to_string(),
                    )),
                }),
        }
    }
}

fn is_valid_buffer(buffer: &[u8], expected_length: usize) -> Result<()> {
    let current_buffer_len = buffer.len();
    if current_buffer_len != expected_length {
        return Err(Error::InvalidEfiVarLen {
            expected: expected_length,
            actual: current_buffer_len,
        });
    }

    Ok(())
}
