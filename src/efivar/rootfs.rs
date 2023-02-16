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
    is_valid_buffer, EfiVar, ROOTFS_STATUS_NORMAL, ROOTFS_STATUS_UNBOOTABLE,
    ROOTFS_STATUS_UPD_DONE, ROOTFS_STATUS_UPD_IN_PROCESS,
};
use super::{SLOT_A, SLOT_B};
use crate::Error;

pub const PATH_STATUS_A: &str =
    "/sys/firmware/efi/efivars/RootfsStatusSlotA-781e084c-a330-417c-b678-38e696380cb9";
pub const PATH_STATUS_B: &str =
    "/sys/firmware/efi/efivars/RootfsStatusSlotB-781e084c-a330-417c-b678-38e696380cb9";
pub const PATH_RETRY_COUNT_A: &str =
    "/sys/firmware/efi/efivars/RootfsRetryCountA-781e084c-a330-417c-b678-38e696380cb9";
pub const PATH_RETRY_COUNT_B: &str =
    "/sys/firmware/efi/efivars/RootfsRetryCountB-781e084c-a330-417c-b678-38e696380cb9";
pub const PATH_RETRY_COUNT_MAX: &str =
    "/sys/firmware/efi/efivars/RootfsRetryCountMax-781e084c-a330-417c-b678-38e696380cb9";

const EXPECTED_LEN: usize = 8;

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

/// Throws an `Error` if the given retry count is exceeding the maximum.
fn is_valid_retry_count(count: u8) -> Result<(), Error> {
    let max_count = get_max_retry_count()?;
    if count > max_count {
        return Err(Error::ExceedingRetryCount {
            counter: count,
            max: max_count,
        });
    }
    Ok(())
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

/// Get the raw rootfs status for a certain `slot`.
pub fn get_rootfs_status(slot: u8) -> Result<u8, Error> {
    let efivar = match slot {
        SLOT_A => EfiVar::open(PATH_STATUS_A, EXPECTED_LEN)?,
        SLOT_B => EfiVar::open(PATH_STATUS_B, EXPECTED_LEN)?,
        _ => return Err(Error::InvalidSlotData),
    };
    let status = parse_buffer(&efivar.buffer)?;
    is_valid_rootfs_status(status)?;
    Ok(status)
}

/// Get the retry count for a certain `slot`.
pub fn get_retry_count(slot: u8) -> Result<u8, Error> {
    let efivar = match slot {
        SLOT_A => EfiVar::open(PATH_RETRY_COUNT_A, EXPECTED_LEN)?,
        SLOT_B => EfiVar::open(PATH_RETRY_COUNT_B, EXPECTED_LEN)?,
        _ => return Err(Error::InvalidSlotData),
    };
    let retry_count = parse_buffer(&efivar.buffer)?;
    is_valid_retry_count(retry_count)?;
    Ok(retry_count)
}

/// Get the maximum retry count.
pub fn get_max_retry_count() -> Result<u8, Error> {
    let efivar = EfiVar::open(PATH_RETRY_COUNT_MAX, EXPECTED_LEN)?;
    parse_buffer(&efivar.buffer)
}

/// Set raw rootfs `status` for a certain `slot`.
pub fn set_rootfs_status(status: u8, slot: u8) -> Result<(), Error> {
    is_valid_rootfs_status(status)?;
    let mut efivar = match slot {
        SLOT_A => EfiVar::open(PATH_STATUS_A, EXPECTED_LEN)?,
        SLOT_B => EfiVar::open(PATH_STATUS_B, EXPECTED_LEN)?,
        _ => return Err(Error::InvalidSlotData),
    };
    set_value_in_buffer(&mut efivar.buffer, status)?;
    efivar.write()
}

/// Set the retry `counter` for a certain `slot`.
pub fn set_retry_count(counter: u8, slot: u8) -> Result<(), Error> {
    is_valid_retry_count(counter)?;
    let mut efivar = match slot {
        SLOT_A => EfiVar::open(PATH_RETRY_COUNT_A, EXPECTED_LEN)?,
        SLOT_B => EfiVar::open(PATH_RETRY_COUNT_B, EXPECTED_LEN)?,
        _ => return Err(Error::InvalidSlotData),
    };
    set_value_in_buffer(&mut efivar.buffer, counter)?;
    efivar.write()
}

#[cfg(test)]
mod tests {
    // Unit testing only buffer based operations.
    use eyre::Result;

    use super::*;

    const ROOTFS_STATUS_NORMAL_DATA: [u8; 8] = [0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    const ROOTFS_STATUS_UPD_IN_PROCESS_DATA: [u8; 8] =
        [0x07, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00];
    const ROOTFS_STATUS_UPD_DONE_DATA: [u8; 8] = [0x07, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00];
    const ROOTFS_STATUS_UNBOOTABLE_DATA: [u8; 8] = [0x07, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00];

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

    const RETRY_COUNT_0_DATA: [u8; 8] = [0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    const RETRY_COUNT_1_DATA: [u8; 8] = [0x07, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00];
    const RETRY_COUNT_2_DATA: [u8; 8] = [0x07, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00];
    const RETRY_COUNT_3_DATA: [u8; 8] = [0x07, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00];

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
        for original_data in 0..3_usize {
            for new_status in 0..3_usize {
                let mut buffer = Vec::from(test_data[original_data].1);
                set_value_in_buffer(&mut buffer, test_data[new_status].0)?;
                let data_status = parse_buffer(&buffer)?;
                assert_eq!(
                    new_status as u8, data_status,
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
        for original_retry in 0..3_usize {
            for new_retry in 0..3_u8 {
                let mut buffer = Vec::from(test_data[original_retry]);
                set_value_in_buffer(&mut buffer, new_retry)?;
                let data_counter = parse_buffer(&buffer)?;
                assert_eq!(new_retry, data_counter, "Retry count unexpected after set");
            }
        }
        Ok(())
    }
}
