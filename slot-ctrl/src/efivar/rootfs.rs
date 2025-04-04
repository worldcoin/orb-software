//! `Rootfs` efivars.
//!
//! * `RootfsStatusSlotA` - represents the status of the rootfs in slot A
//! * `RootfsStatusSlotB` - represents the status of the rootfs in slot B
//!
//! Bits of interest are found in byte 4 for all efivars.

use super::{
    is_valid_buffer, EfiVar, EfiVarDb, EfiVarDbErr, ROOTFS_STATUS_NORMAL,
    ROOTFS_STATUS_UNBOOTABLE, ROOTFS_STATUS_UPD_DONE, ROOTFS_STATUS_UPD_IN_PROCESS,
};
use super::{SLOT_A, SLOT_B};
use crate::Error;

const PATH_STATUS_A: &str = "RootfsStatusSlotA-781e084c-a330-417c-b678-38e696380cb9";
const PATH_STATUS_B: &str = "RootfsStatusSlotB-781e084c-a330-417c-b678-38e696380cb9";

const EXPECTED_LEN: usize = 8;

pub struct RootfsEfiVars {
    pub(crate) status_a: EfiVar,
    pub(crate) status_b: EfiVar,
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
fn parse_buffer(buffer: &[u8]) -> Result<u8, Error> {
    is_valid_buffer(buffer, EXPECTED_LEN)?;
    Ok(buffer[4])
}

// Set the value in a `buffer`s byte 4.
fn set_value_in_buffer(buffer: &mut Vec<u8>, value: u8) -> Result<(), Error> {
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
        })
    }

    /// Get the raw rootfs status for a certain `slot`.
    pub fn get_rootfs_status(&self, slot: u8) -> Result<u8, Error> {
        let efivar = match slot {
            SLOT_A => &self.status_a,
            SLOT_B => &self.status_b,
            _ => return Err(Error::InvalidSlotData),
        };

        let status = parse_buffer(&efivar.read_fixed_len(EXPECTED_LEN)?)?;
        is_valid_rootfs_status(status)?;

        Ok(status)
    }

    /// Set raw rootfs `status` for a certain `slot`.
    pub fn set_rootfs_status(&self, status: u8, slot: u8) -> Result<(), Error> {
        is_valid_rootfs_status(status)?;
        let efivar = match slot {
            SLOT_A => &self.status_a,
            SLOT_B => &self.status_b,
            _ => return Err(Error::InvalidSlotData),
        };

        let mut buf = efivar.read_fixed_len(EXPECTED_LEN)?;
        set_value_in_buffer(&mut buf, status)?;
        efivar.write(&buf)
    }
}

#[cfg(test)]
mod tests {
    // Unit testing only buffer based operations.
    use eyre::Result;

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
}