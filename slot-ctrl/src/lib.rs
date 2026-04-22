//! API for managing slot switching and status.
pub use domain::{BootChainFwStatus, EfiRetryCount, Error, Result, RootFsStatus, Slot};
use domain::{RetryCounts, ScratchRegRetryCount};
use efivar::{EfiVar, EfiVarDb};
use orb_info::orb_os_release::OrbOsPlatform;
use std::{
    fs::{self},
    path::{Path, PathBuf},
};

mod domain;

pub mod program;
pub mod test_utils;

pub struct OrbSlotCtrl {
    orb_type: OrbOsPlatform,
    rootfs: PathBuf,
    current_slot: EfiVar,
    next_slot: EfiVar,
    status_a: EfiVar,
    status_b: EfiVar,
    efi_retry_count_a: EfiVar,
    efi_retry_count_b: EfiVar,
    efi_retry_count_max: EfiVar,
    reg_retry_count_a: PathBuf,
    reg_retry_count_b: PathBuf,
    bootchain_fw_status: EfiVar,
}

impl OrbSlotCtrl {
    pub fn new(rootfs: impl AsRef<Path>, orb_type: OrbOsPlatform) -> Result<Self> {
        let rootfs = rootfs.as_ref().to_path_buf();
        let db = EfiVarDb::from_rootfs(&rootfs)?;

        let (reg_path_a, reg_path_b) = match orb_type {
            OrbOsPlatform::Diamond => (
                ScratchRegRetryCount::DIAMOND_REG_PATH_A,
                ScratchRegRetryCount::DIAMOND_REG_PATH_B,
            ),
            OrbOsPlatform::Pearl => (
                ScratchRegRetryCount::PEARL_REG_PATH_A,
                ScratchRegRetryCount::PEARL_REG_PATH_B,
            ),
        };

        Ok(Self {
            orb_type,
            rootfs,
            reg_retry_count_a: reg_path_a.into(),
            reg_retry_count_b: reg_path_b.into(),
            next_slot: db.get_var(Slot::NEXT_SLOT_PATH)?,
            current_slot: db.get_var(Slot::CURRENT_SLOT_PATH)?,
            status_a: db.get_var(RootFsStatus::STATUS_A_PATH)?,
            status_b: db.get_var(RootFsStatus::STATUS_B_PATH)?,
            efi_retry_count_a: db.get_var(EfiRetryCount::COUNT_A_PATH)?,
            efi_retry_count_b: db.get_var(EfiRetryCount::COUNT_B_PATH)?,
            efi_retry_count_max: db.get_var(EfiRetryCount::COUNT_MAX_PATH)?,
            bootchain_fw_status: db.get_var(BootChainFwStatus::STATUS_PATH)?,
        })
    }

    pub fn read_bootchain_fw_status(&self) -> Result<BootChainFwStatus> {
        BootChainFwStatus::from_efivar_data(&self.bootchain_fw_status.read()?)
    }

    pub fn set_bootchain_fw_status(&self, status: BootChainFwStatus) -> Result<()> {
        self.bootchain_fw_status.write(&status.to_efivar_data())?;

        Ok(())
    }

    pub fn delete_bootchain_fw_status(&self) -> Result<()> {
        self.bootchain_fw_status.remove()?;
        Ok(())
    }

    /// Get the current active slot.
    pub fn get_current_slot(&self) -> Result<Slot> {
        let data = self.current_slot.read()?;
        Slot::from_efivar_data(&data)
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
            .and_then(|data| Slot::from_efivar_data(&data))
            .or_else(|_| self.get_current_slot())
    }

    /// Set the slot for the next boot.
    pub fn set_next_boot_slot(&self, slot: Slot) -> Result<()> {
        self.mark_slot_ok(slot)?;
        self.next_slot.write(&slot.to_efivar_data())?;

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

        let data = status_var.read()?;
        RootFsStatus::from_efivar_data(&data)
    }

    /// Set a rootfs status for a certain `slot`.
    pub fn set_rootfs_status(&self, status: RootFsStatus, slot: Slot) -> Result<()> {
        let status_var = match slot {
            Slot::A => &self.status_a,
            Slot::B => &self.status_b,
        };

        status_var.write(&status.to_efivar_data())?;
        Ok(())
    }

    pub(crate) fn get_retry_counts(&self, slot: Slot) -> Result<RetryCounts> {
        let (efi_var_path, sr_rf_path) = match slot {
            Slot::A => (&self.efi_retry_count_a, &self.reg_retry_count_a),
            Slot::B => (&self.efi_retry_count_b, &self.reg_retry_count_b),
        };

        let efi_var_data = &efi_var_path.read()?;
        let efi_retry_count = EfiRetryCount::from_efivar_data(efi_var_data)?;
        let reg_retry_count = ScratchRegRetryCount::new(self.rootfs.join(sr_rf_path))?;

        Ok(RetryCounts {
            efi_var: efi_retry_count,
            sr_rf: reg_retry_count,
        })
    }

    /// Get the maximum EFI retry count before fallback.
    pub(crate) fn get_efi_max_retry_count(&self) -> Result<EfiRetryCount> {
        EfiRetryCount::from_efivar_data(&self.efi_retry_count_max.read()?)
    }

    /// Reset the EFI retry counter to the maximum for the given `slot`.
    pub(crate) fn reset_efi_retry_count_to_max(&self, slot: Slot) -> Result<()> {
        let max_count = self.get_efi_max_retry_count()?;
        self.set_efivar_retry_count(slot, max_count)
    }

    /// Reset the SR_RF retry counter to the maximum for the given `slot`.
    pub(crate) fn reset_srrf_retry_count_to_max(&self, slot: Slot) -> Result<()> {
        self.set_srrf_retry_count(slot, ScratchRegRetryCount::SR_RF_COUNT_MAX)
    }

    /// Marks the current slot as working correctly so that
    /// Nvidia slot A/B switching redundancy mechanism knows that this boot was successful
    pub fn mark_current_slot_ok(&self) -> Result<()> {
        self.mark_slot_ok(self.get_current_slot()?)
    }

    ///  Marking slot as ok:
    ///  1) resets the retry count on the efivars
    ///  2) resets the retry count on SR_RF
    ///  3) marks the rootfs slot status as Normal
    ///  4) removes BootChainFwStatus if present
    pub fn mark_slot_ok(&self, slot: Slot) -> Result<()> {
        self.reset_efi_retry_count_to_max(slot)?;
        self.reset_srrf_retry_count_to_max(slot)?;
        self.set_rootfs_status(RootFsStatus::Normal, slot)?;
        self.bootchain_fw_status.remove()?;
        Ok(())
    }

    fn set_efivar_retry_count(&self, slot: Slot, val: EfiRetryCount) -> Result<()> {
        let efivar = match slot {
            Slot::A => &self.efi_retry_count_a,
            Slot::B => &self.efi_retry_count_b,
        };

        // on a cold reboot the efivars will be used to initialize the SR_RF.
        // the RootfsRetryCountX value is bounded by the max value of SR_RF
        if val.0 > ScratchRegRetryCount::SR_RF_COUNT_MAX {
            return Err(Error::ExceedingRetryCount {
                counter: val.0,
                max: ScratchRegRetryCount::SR_RF_COUNT_MAX,
            });
        }

        efivar.write(&val.to_efivar_data())?;

        Ok(())
    }

    fn set_srrf_retry_count(&self, slot: Slot, val: u8) -> Result<()> {
        // SR_RF retry count is stored in a scratch register
        // `tegra-pmc` driver exposes control over a sysfs device node

        if val > ScratchRegRetryCount::SR_RF_COUNT_MAX {
            return Err(Error::ExceedingRetryCount {
                counter: val,
                max: ScratchRegRetryCount::SR_RF_COUNT_MAX,
            });
        }

        let reg_path = match slot {
            Slot::A => &self.reg_retry_count_a,
            Slot::B => &self.reg_retry_count_b,
        };

        fs::write(
            self.rootfs.join(reg_path),
            format!("0x{:x}", val).as_bytes(),
        )
        .map_err(|e| Error::CouldNotOpenScratchReg(e.to_string()))?;

        Ok(())
    }
}
