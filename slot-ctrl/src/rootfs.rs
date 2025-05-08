//! `Rootfs` efivars.
//!
//! * `RootfsStatusSlotA` - represents the status of the rootfs in slot A
//! * `RootfsStatusSlotB` - represents the status of the rootfs in slot B
//! * `RootfsRetryCountA` - represents the boot retry count of the rootfs in slot A
//! * `RootfsRetryCountB` - represents the boot retry count of the rootfs in slot B
//! * `RootfsRetryCountMax` - represents the maximum boot retry count
//!
//! Bits of interest are found in byte 4 for all efivars.

use super::{
    is_valid_buffer, ROOTFS_STATUS_NORMAL, ROOTFS_STATUS_UNBOOTABLE,
    ROOTFS_STATUS_UPD_DONE, ROOTFS_STATUS_UPD_IN_PROCESS,
};

use color_eyre::{eyre::eyre, Result};
use efivar::{EfiVar, EfiVarDb, EfiVarDbErr};

use super::{SLOT_A, SLOT_B};
use crate::Error;

const PATH_STATUS_A: &str = "RootfsStatusSlotA-781e084c-a330-417c-b678-38e696380cb9";
const PATH_STATUS_B: &str = "RootfsStatusSlotB-781e084c-a330-417c-b678-38e696380cb9";
const PATH_RETRY_COUNT_A: &str =
    "RootfsRetryCountA-781e084c-a330-417c-b678-38e696380cb9";
const PATH_RETRY_COUNT_B: &str =
    "RootfsRetryCountB-781e084c-a330-417c-b678-38e696380cb9";
const PATH_RETRY_COUNT_MAX: &str =
    "RootfsRetryCountMax-781e084c-a330-417c-b678-38e696380cb9";

const EXPECTED_LEN: usize = 8;

pub struct RootfsEfiVars {
    pub(crate) status_a: EfiVar,
    pub(crate) status_b: EfiVar,
    pub(crate) retry_count_a: EfiVar,
    pub(crate) retry_count_b: EfiVar,
    pub(crate) retry_count_max: EfiVar,
}

/// Throws an `Error` if the given rootfs status is invalid.
fn is_valid_rootfs_status(status: u8) -> Result<(), Error> {
    match status {
        ROOTFS_STATUS_NORMAL
        | ROOTFS_STATUS_UPD_IN_PROCESS
        | ROOTFS_STATUS_UPD_DONE
        | ROOTFS_STATUS_UNBOOTABLE => Ok(()),
        _ => Err(Error::InvalidRootFsStatusData),
    }
}

// Get the information of interest from a `buffer`s byte 4.
fn parse_buffer(buffer: &[u8]) -> Result<u8> {
    is_valid_buffer(buffer, EXPECTED_LEN)?;
    Ok(buffer[4])
}

// Set the value in a `buffer`s byte 4.
fn set_value_in_buffer(buffer: &mut Vec<u8>, value: u8) -> Result<()> {
    is_valid_buffer(&*buffer, EXPECTED_LEN)?;
    buffer[4] = value;
    Ok(())
}

impl RootfsEfiVars {
    /// Creates a RootfsEfiVars
    pub fn new(db: &EfiVarDb) -> Result<Self, EfiVarDbErr> {
        Ok(Self {
            status_a: db.get_var(PATH_STATUS_A)?,
            status_b: db.get_var(PATH_STATUS_B)?,
            retry_count_a: db.get_var(PATH_RETRY_COUNT_A)?,
            retry_count_b: db.get_var(PATH_RETRY_COUNT_B)?,
            retry_count_max: db.get_var(PATH_RETRY_COUNT_MAX)?,
        })
    }

    /// Get the raw rootfs status for a certain `slot`.
    pub fn get_rootfs_status(&self, slot: u8) -> Result<u8> {
        let efivar = match slot {
            SLOT_A => &self.status_a,
            SLOT_B => &self.status_b,
            _ => return Err(eyre!("Invalid slot data")),
        };

        let status = parse_buffer(&efivar.read()?)?;
        is_valid_rootfs_status(status)?;

        Ok(status)
    }

    /// Get the retry count for a certain `slot`.
    pub fn get_retry_count(&self, slot: u8) -> Result<u8> {
        let efivar = match slot {
            SLOT_A => &self.retry_count_a,
            SLOT_B => &self.retry_count_b,
            _ => return Err(eyre!("Invalid slot data")),
        };

        let retry_count = parse_buffer(&efivar.read()?)?;
        self.is_valid_retry_count(retry_count)?;

        Ok(retry_count)
    }

    /// Get the maximum retry count.
    pub fn get_max_retry_count(&self) -> Result<u8> {
        parse_buffer(&self.retry_count_max.read()?)
    }

    /// Set raw rootfs `status` for a certain `slot`.
    pub fn set_rootfs_status(&self, status: u8, slot: u8) -> Result<()> {
        is_valid_rootfs_status(status)?;
        let efivar = match slot {
            SLOT_A => &self.status_a,
            SLOT_B => &self.status_b,
            _ => return Err(eyre!("Invalid slot data")),
        };

        let mut buf = efivar.read()?;
        set_value_in_buffer(&mut buf, status)?;
        efivar.write(&buf)
    }

    /// Set the retry `counter` for a certain `slot`.
    pub fn set_retry_count(&self, counter: u8, slot: u8) -> Result<()> {
        self.is_valid_retry_count(counter)?;
        let efivar = match slot {
            SLOT_A => &self.retry_count_a,
            SLOT_B => &self.retry_count_b,
            _ => return Err(eyre!("Invalid slot data")),
        };

        let mut buf = efivar.read()?;
        set_value_in_buffer(&mut buf, counter)?;
        efivar.write(&buf)
    }

    /// Throws an `Error` if the given retry count is exceeding the maximum.
    fn is_valid_retry_count(&self, count: u8) -> Result<()> {
        let max_count = self.get_max_retry_count()?;
        if count > max_count {
            return Err(eyre!(
                "Exeeding retry count: counter [count], max [max_count]"
            ));
        };

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    // Unit testing only buffer based operations.
    use color_eyre::eyre::Result;

    use super::*;

    const ROOTFS_STATUS_NORMAL_DATA: [u8; 8] =
        [0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    const ROOTFS_STATUS_UPD_IN_PROCESS_DATA: [u8; 8] =
        [0x07, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00];
    const ROOTFS_STATUS_UPD_DONE_DATA: [u8; 8] =
        [0x07, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00];
    const ROOTFS_STATUS_UNBOOTABLE_DATA: [u8; 8] =
        [0x07, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00];

    fn assert_rootfs_status(status: u8, data: [u8; 8]) -> Result<()> {
        let buffer = Vec::from(data);
        let read_status = parse_buffer(&buffer)?;
        assert_eq!(read_status, status, "Read unexpected rootfs status");
        Ok(())
    }

    // Test reading rootfs status from accordingly configured buffers.
    #[test]
    fn get_current_rootfs_status() -> Result<()> {
        assert_rootfs_status(ROOTFS_STATUS_NORMAL, ROOTFS_STATUS_NORMAL_DATA)?;
        assert_rootfs_status(
            ROOTFS_STATUS_UPD_IN_PROCESS,
            ROOTFS_STATUS_UPD_IN_PROCESS_DATA,
        )?;
        assert_rootfs_status(ROOTFS_STATUS_UPD_DONE, ROOTFS_STATUS_UPD_DONE_DATA)?;
        assert_rootfs_status(ROOTFS_STATUS_UNBOOTABLE, ROOTFS_STATUS_UNBOOTABLE_DATA)?;
        Ok(())
    }

    const RETRY_COUNT_0_DATA: [u8; 8] =
        [0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    const RETRY_COUNT_1_DATA: [u8; 8] =
        [0x07, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00];
    const RETRY_COUNT_2_DATA: [u8; 8] =
        [0x07, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00];
    const RETRY_COUNT_3_DATA: [u8; 8] =
        [0x07, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00];

    fn assert_retry_count(count: u8, data: [u8; 8]) -> Result<()> {
        let buffer = Vec::from(data);
        let current_count = parse_buffer(&buffer)?;
        assert_eq!(current_count, count, "Read unexpected retry count");
        Ok(())
    }

    // Test reading certain retry count from accordingly configured buffers.
    #[test]
    fn test_get_current_retry_count() -> Result<()> {
        assert_retry_count(0, RETRY_COUNT_0_DATA)?;
        assert_retry_count(1, RETRY_COUNT_1_DATA)?;
        assert_retry_count(2, RETRY_COUNT_2_DATA)?;
        assert_retry_count(3, RETRY_COUNT_3_DATA)?;
        Ok(())
    }

    // test setting rootfs status.
    #[test]
    fn test_set_rootfs_status() -> Result<()> {
        let test_data = [
            (ROOTFS_STATUS_NORMAL, ROOTFS_STATUS_NORMAL_DATA),
            (
                ROOTFS_STATUS_UPD_IN_PROCESS,
                ROOTFS_STATUS_UPD_IN_PROCESS_DATA,
            ),
            (ROOTFS_STATUS_UPD_DONE, ROOTFS_STATUS_UPD_DONE_DATA),
            (ROOTFS_STATUS_UNBOOTABLE, ROOTFS_STATUS_UNBOOTABLE_DATA),
        ];
        for (_, original_data) in test_data {
            for (new_status, _) in test_data {
                let mut buffer = Vec::from(original_data);
                set_value_in_buffer(&mut buffer, new_status)?;
                let data_status = parse_buffer(&buffer)?;
                assert_eq!(
                    new_status, data_status,
                    "Rootfs status unexpected after set"
                );
            }
        }
        Ok(())
    }

    // Test setting retry count.
    #[test]
    fn test_set_retry_count() -> Result<()> {
        let test_data = [
            RETRY_COUNT_0_DATA,
            RETRY_COUNT_1_DATA,
            RETRY_COUNT_2_DATA,
            RETRY_COUNT_3_DATA,
        ];
        for original_data in test_data {
            for new_retry in 0..3_u8 {
                let mut buffer = Vec::from(original_data);
                set_value_in_buffer(&mut buffer, new_retry)?;
                let data_counter = parse_buffer(&buffer)?;
                assert_eq!(new_retry, data_counter, "Retry count unexpected after set");
            }
        }
        Ok(())
    }
}
