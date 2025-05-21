//! API for managing slot switching and status.
pub use domain::{Error, Result, RetryCount, RootFsStatus, Slot};
use efivar::{EfiVar, EfiVarDb};
use orb_info::orb_os_release::OrbType;
use std::path::Path;

mod domain;

pub mod program;
pub mod test_utils;

pub struct OrbSlotCtrl {
    orb_type: OrbType,
    current_slot: EfiVar,
    next_slot: EfiVar,
    status_a: EfiVar,
    status_b: EfiVar,
    retry_count_a: EfiVar,
    retry_count_b: EfiVar,
    retry_count_max: EfiVar,
}

impl OrbSlotCtrl {
    pub fn new(rootfs: impl AsRef<Path>, orb_type: OrbType) -> Result<Self> {
        let efivardb = EfiVarDb::from_rootfs(rootfs)?;

        OrbSlotCtrl::from_evifar_db(&efivardb, orb_type)
    }

    pub fn from_evifar_db(db: &EfiVarDb, orb_type: OrbType) -> Result<Self> {
        Ok(Self {
            orb_type,
            current_slot: db.get_var(Slot::CURRENT_SLOT_PATH)?,
            next_slot: db.get_var(Slot::NEXT_SLOT_PATH)?,
            status_a: db.get_var(RootFsStatus::STATUS_A_PATH)?,
            status_b: db.get_var(RootFsStatus::STATUS_B_PATH)?,
            retry_count_a: db.get_var(RootFsStatus::RETRY_COUNT_A_PATH)?,
            retry_count_b: db.get_var(RootFsStatus::RETRY_COUNT_B_PATH)?,
            retry_count_max: db.get_var(RootFsStatus::RETRY_COUNT_MAX_PATH)?,
        })
    }

    /// Get the current active slot.
    pub fn get_current_slot(&self) -> Result<Slot> {
        let buf = self.current_slot.read()?;
        Slot::from_bytes(buf.as_slice())
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
        self.next_slot
            .read()
            .map_err(Error::EfiVar)
            .and_then(Slot::from_bytes)
            .or_else(|_| self.get_current_slot())
    }

    /// Set the slot for the next boot.
    pub fn set_next_boot_slot(&self, slot: Slot) -> Result<()> {
        self.mark_slot_ok(slot)?;
        self.next_slot.write(slot.as_bytes())?;
        Ok(())
    }

    /// Get the rootfs status for the current active slot.
    pub fn get_current_rootfs_status(&self) -> Result<RootFsStatus> {
        self.get_rootfs_status(self.get_current_slot()?)
    }

    /// Get the rootfs status for a certain `slot`.
    pub fn get_rootfs_status(&self, slot: Slot) -> Result<RootFsStatus> {
        let status_var = match slot {
            Slot::A => &self.status_a,
            Slot::B => &self.status_b,
        };

        let buf = status_var.read()?;
        RootFsStatus::from_bytes(&buf, self.orb_type)
    }

    /// Set a rootfs status for the current active slot.
    pub fn set_current_rootfs_status(&self, status: RootFsStatus) -> Result<()> {
        self.set_rootfs_status(status, self.get_current_slot()?)
    }

    /// Set a rootfs status for a certain `slot`.
    pub fn set_rootfs_status(&self, status: RootFsStatus, slot: Slot) -> Result<()> {
        let status_var = match slot {
            Slot::A => &self.status_a,
            Slot::B => &self.status_b,
        };

        status_var.write(status.as_bytes(self.orb_type)?)?;

        Ok(())
    }

    /// Get the retry count for the current active slot.
    pub(crate) fn get_current_retry_count(&self) -> Result<RetryCount> {
        self.get_retry_count(self.get_current_slot()?)
    }

    /// Get the retry count for a certain `slot`.
    pub(crate) fn get_retry_count(&self, slot: Slot) -> Result<RetryCount> {
        if self.orb_type != OrbType::Pearl {
            return Err(Error::UnsupportedOrbType(self.orb_type));
        }

        let efivar = match slot {
            Slot::A => &self.retry_count_a,
            Slot::B => &self.retry_count_b,
        };

        RetryCount::from_bytes(&efivar.read()?)
    }

    /// Get the maximum retry count before fallback.
    pub(crate) fn get_max_retry_count(&self) -> Result<RetryCount> {
        RetryCount::from_bytes(&self.retry_count_max.read()?)
    }

    /// Reset the retry counter to the maximum for the current active slot.
    pub(crate) fn reset_current_retry_count_to_max(&self) -> Result<()> {
        self.reset_retry_count_to_max(self.get_current_slot()?)
    }

    /// Reset the retry counter to the maximum for the a certain `slot`.
    pub(crate) fn reset_retry_count_to_max(&self, slot: Slot) -> Result<()> {
        if self.orb_type != OrbType::Pearl {
            return Err(Error::UnsupportedOrbType(self.orb_type));
        }

        let max_count = self.get_max_retry_count()?;
        self.set_retry_count(slot, max_count)
    }

    /// Marks the current slot as working correctly so that
    /// Nvidia slot A/B switching redundancy mechanizm knows that this boot was successful
    pub fn mark_current_slot_ok(&self) -> Result<()> {
        self.mark_slot_ok(self.get_current_slot()?)
    }

    pub fn mark_slot_ok(&self, slot: Slot) -> Result<()> {
        // Theoretically we never reach this point if the slot is not Normal
        // But on Pearl we have 2 more states: UpdateDone + UpdateInProgress
        // TODO: Remove this once these 2 extra states are removed from edk2
        self.set_current_rootfs_status(RootFsStatus::Normal)?;

        match self.orb_type {
            OrbType::Pearl => self.reset_retry_count_to_max(slot),
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

    fn set_retry_count(&self, slot: Slot, val: RetryCount) -> Result<()> {
        let efivar = match slot {
            Slot::A => &self.retry_count_a,
            Slot::B => &self.retry_count_b,
        };

        efivar.write(&val.as_bytes())?;

        Ok(())
    }
}
