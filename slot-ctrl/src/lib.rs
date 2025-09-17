//! API for managing slot switching and status.
pub use domain::{BootChainFwStatus, EfiRetryCount, Error, Result, RootFsStatus, Slot};
use domain::{RetryCounts, ScratchRegRetryCount};
use efivar::{EfiVar, EfiVarDb};
use orb_info::orb_os_release::OrbOsPlatform;
use std::{
    fs,
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
    retry_count_a: EfiVar,
    retry_count_b: EfiVar,
    retry_count_max: EfiVar,
    bootchain_fw_status: EfiVar,
}

impl OrbSlotCtrl {
    pub fn new(rootfs: impl AsRef<Path>, orb_type: OrbOsPlatform) -> Result<Self> {
        let rootfs = rootfs.as_ref().to_path_buf();
        let db = EfiVarDb::from_rootfs(&rootfs)?;

        Ok(Self {
            orb_type,
            rootfs,
            current_slot: db.get_var(Slot::CURRENT_SLOT_PATH)?,
            next_slot: db.get_var(Slot::NEXT_SLOT_PATH)?,
            status_a: db.get_var(RootFsStatus::STATUS_A_PATH)?,
            status_b: db.get_var(RootFsStatus::STATUS_B_PATH)?,
            retry_count_a: db.get_var(EfiRetryCount::COUNT_A_PATH)?,
            retry_count_b: db.get_var(EfiRetryCount::COUNT_B_PATH)?,
            retry_count_max: db.get_var(EfiRetryCount::COUNT_MAX_PATH)?,
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
        RootFsStatus::from_efivar_data(&data, self.orb_type)
    }

    /// Set a rootfs status for a certain `slot`.
    pub fn set_rootfs_status(&self, status: RootFsStatus, slot: Slot) -> Result<()> {
        let status_var = match slot {
            Slot::A => &self.status_a,
            Slot::B => &self.status_b,
        };

        status_var.write(&status.to_efivar_data(self.orb_type)?)?;

        Ok(())
    }

    /// Gets both the EFI and the scratch register retry counters
    pub(crate) fn get_retry_counts(&self, slot: Slot) -> Result<RetryCounts> {
        let (pearl_efi_var, diamond_scratch_register_path) = match slot {
            Slot::A => (
                &self.retry_count_a,
                self.rootfs.join(ScratchRegRetryCount::DIAMOND_COUNT_A_PATH),
            ),

            Slot::B => (
                &self.retry_count_b,
                self.rootfs.join(ScratchRegRetryCount::DIAMOND_COUNT_B_PATH),
            ),
        };

        let (efi_var, scratch_reg) = match self.orb_type {
            OrbOsPlatform::Diamond => {
                let count = fs::read(diamond_scratch_register_path)
                    .map_err(|e| Error::CouldNotReadScratchReg(e.to_string()))?;

                let count = String::from_utf8(count)
                    .map_err(|e| Error::CouldNotReadScratchReg(e.to_string()))?;

                let count = count.strip_prefix("0x").ok_or_else(|| {
                    Error::CouldNotReadScratchReg(format!(
                        "scratch register retry count in unexpected format {count}"
                    ))
                })?;

                let count = count.trim().parse::<u8>().map_err(|e| {
                    Error::CouldNotReadScratchReg(format!("{e}: '{count}'"))
                })?;

                let count = ScratchRegRetryCount(count);

                (None, Some(count))
            }

            OrbOsPlatform::Pearl => {
                let efi_var_data = &pearl_efi_var.read()?;
                let count = EfiRetryCount::from_efivar_data(efi_var_data)?;

                (Some(count), None)
            }
        };

        Ok(RetryCounts {
            efi_var,
            scratch_reg,
        })
    }

    /// Get the maximum EFI retry count before fallback.
    pub(crate) fn get_efi_max_retry_count(&self) -> Result<EfiRetryCount> {
        EfiRetryCount::from_efivar_data(&self.retry_count_max.read()?)
    }

    /// Reset the EFI retry counter to the maximum for the a certain `slot`.
    pub(crate) fn reset_efi_retry_count_to_max(&self, slot: Slot) -> Result<()> {
        if self.orb_type != OrbOsPlatform::Pearl {
            return Err(Error::UnsupportedOrbType(self.orb_type));
        }

        let max_count = self.get_efi_max_retry_count()?;
        self.set_retry_count(slot, max_count)
    }

    /// Marks the current slot as working correctly so that
    /// Nvidia slot A/B switching redundancy mechanism knows that this boot was successful
    pub fn mark_current_slot_ok(&self) -> Result<()> {
        self.mark_slot_ok(self.get_current_slot()?)
    }

    pub fn mark_slot_ok(&self, slot: Slot) -> Result<()> {
        self.set_rootfs_status(RootFsStatus::Normal, slot)?;

        match self.orb_type {
            OrbOsPlatform::Pearl => {
                // We never reach this point in the code if the slot is Unbootable
                // On Pearl we have 2 more states: UpdateDone + UpdateInProgress
                // TODO: Remove this once these 2 extra states are removed from edk2
                // We need this because we use an out of band mechanism for slot integrity.
                // (nvidia does not use EfiVars for the retry count, yet we created our own
                // to get around this in Pearl)
                // No one to ask for context about this -- everyone involved has already left :D
                self.reset_efi_retry_count_to_max(slot)
            }

            OrbOsPlatform::Diamond => {
                if let Ok(efivar) = self.bootchain_fw_status.read() {
                    // We introduced a change on Diamond EDK2 that made it so that we cannot switch slots if this
                    // variable is present in userspace. It is typically present with 0x7 EfiVar attributes, and the values 0000,
                    // which signifies a successful BootChainFw update. We don't know why this is the case,
                    // but deleting it makes slot switching work. If we don't delete it, orb will power cycle
                    // successfully but will remain in the same slot.
                    // Ask @alekseifedotov or @vmenge about this for more context.
                    println!("BootChainFwStatus efi var found, will remove.");
                    println!(
                        "EfiVar to be removed: {:?}\n{efivar}",
                        self.bootchain_fw_status.path()
                    );
                    self.bootchain_fw_status.remove()?;
                }

                // We don't do anything else here because marking slot as ok is handled on Diamond by:
                // /opt/nvidia/l4t-rootfs-validation-config
                // /opt/nvidia/l4t-bootloader-config
                // Once or if we remove acccess to /dev/mem, the nvidia services will break and we will
                // need to do it ourselves.

                Ok(())
            }
        }
    }

    fn set_retry_count(&self, slot: Slot, val: EfiRetryCount) -> Result<()> {
        let efivar = match slot {
            Slot::A => &self.retry_count_a,
            Slot::B => &self.retry_count_b,
        };

        efivar.write(&val.to_efivar_data())?;

        Ok(())
    }
}
