//! `RootFSInfo` efivar.
//!
//! Example value for this efivar: [0x07, 0x00, 0x00, 0x00, 0x3c, 0xc0, 0x01, 0x00]
//!
//! Bits of interest are found in byte 4 and 5 (numbering from zero):
//!
//! | Bits  | Variable         | Value |
//! |-------|------------------|-------|
//! | 0-1   | `RootfsStatus_B` | 0b00  |
//! | 2-3   | `RetryCount_A`   | 0b11  |
//! | 4-5   | `RetryCount_B`   | 0b11  |
//! | 6     | `Current_Slot`   | 0b00  |
//! | 7     | `Slot_Link`      | 0b00  |
//! | 8-9   | `MAX_RetryCount` | 0b11  |
//! | 10-11 | `UPD_MODE_B`     | 0b00  |
//! | 12-13 | `UPD_MODE_A`     | 0b00  |
//! | 14-15 | `RootfsStatus_A` | 0b00  |

#[cfg(test)]
use super::{
    ROOTFS_STATUS_NORMAL, ROOTFS_STATUS_UNBOOTABLE, ROOTFS_STATUS_UPD_DONE,
    ROOTFS_STATUS_UPD_IN_PROCESS,
};
use super::{SLOT_A, SLOT_B};
use crate::Error;

#[cfg(not(test))]
pub const PATH: &str = "/sys/firmware/efi/efivars/RootfsInfo-781e084c-a330-417c-b678-38e696380cb9";
#[cfg(test)]
pub const PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/testrootfsefivar");

const EXPECTED_LEN: usize = 8;

/// Bits used by certain variable.
const ROOTFS_STATUS_BITS: u8 = 2;
const RETRY_COUNT_BITS: u8 = 2;
const CURRENT_SLOT_BITS: u8 = 1;
const SLOT_LINK_BITS: u8 = 1;
#[allow(dead_code)]
const MAX_RETRY_COUNT_BITS: u8 = 2;
const UDP_MODE_BITS: u8 = 2;

/// Byte 4 representation.
const SLOT_LINK_SHIFT: u8 = 0;
#[allow(dead_code)]
const SLOT_LINK_MASK: u8 = 0b0000_0001_u8;
const CURRENT_SLOT_SHIFT: u8 = SLOT_LINK_SHIFT + SLOT_LINK_BITS;
const CURRENT_SLOT_MASK: u8 = 0b0000_0010_u8;
const RETRY_COUNT_B_SHIFT: u8 = CURRENT_SLOT_SHIFT + CURRENT_SLOT_BITS;
const RETRY_COUNT_B_MASK: u8 = 0b0000_1100_u8;
const RETRY_COUNT_A_SHIFT: u8 = RETRY_COUNT_B_SHIFT + RETRY_COUNT_BITS;
const RETRY_COUNT_A_MASK: u8 = 0b0011_0000_u8;
const ROOTFS_STATUS_B_SHIFT: u8 = RETRY_COUNT_A_SHIFT + RETRY_COUNT_BITS;
const ROOTFS_STATUS_B_MASK: u8 = 0b1100_0000_u8;

/// Byte 5 representation.
const ROOTFS_STATUS_A_SHIFT: u8 = 0;
const ROOTFS_STATUS_A_MASK: u8 = 0b0000_0011_u8;
const UDP_MODE_A_SHIFT: u8 = ROOTFS_STATUS_A_SHIFT + ROOTFS_STATUS_BITS;
#[allow(dead_code)]
const UDP_MODE_A_MASK: u8 = 0b0000_1100_u8;
const UDP_MODE_B_SHIFT: u8 = UDP_MODE_A_SHIFT + UDP_MODE_BITS;
#[allow(dead_code)]
const UDP_MODE_B_MASK: u8 = 0b0011_0000_u8;
const MAX_RETRY_COUNT_SHIFT: u8 = UDP_MODE_B_SHIFT + UDP_MODE_BITS;
const MAX_RETRY_COUNT_MASK: u8 = 0b1100_0000_u8;

// Get the raw active slot from `buffer`.
fn get_active_slot_from_buffer(buffer: &[u8]) -> Result<u8, Error> {
    super::is_valid_buffer(buffer, EXPECTED_LEN)?;
    // Current slot information can be found in byte 4.
    let byte_4: u8 = buffer[4];
    Ok((CURRENT_SLOT_MASK & byte_4) >> CURRENT_SLOT_SHIFT)
}

// Get the raw rootfs status from `buffer` for a certain `slot`.
fn get_rootfs_status_from_buffer(buffer: &[u8], slot: u8) -> Result<u8, Error> {
    super::is_valid_buffer(buffer, EXPECTED_LEN)?;
    match slot {
        SLOT_A => {
            // Information is stored in byte 5 for Slot A.
            let byte_5: u8 = buffer[5];
            Ok((ROOTFS_STATUS_A_MASK & byte_5) >> ROOTFS_STATUS_A_SHIFT)
        }
        // Slot B.
        SLOT_B => {
            // Information is stored in byte 4 for Slot B.
            let byte_4: u8 = buffer[4];
            Ok((ROOTFS_STATUS_B_MASK & byte_4) >> ROOTFS_STATUS_B_SHIFT)
        }
        _ => Err(Error::InvalidCurrentSlotData),
    }
}

// Get the retry count from `buffer` for a certain `slot`.
fn get_retry_count_from_buffer(buffer: &[u8], slot: u8) -> Result<u8, Error> {
    super::is_valid_buffer(buffer, EXPECTED_LEN)?;
    // Information is stored in byte 4 for both Slots.
    let byte_4: u8 = buffer[4];
    match slot {
        SLOT_A => Ok((RETRY_COUNT_A_MASK & byte_4) >> RETRY_COUNT_A_SHIFT),
        SLOT_B => Ok((RETRY_COUNT_B_MASK & byte_4) >> RETRY_COUNT_B_SHIFT),
        _ => Err(Error::InvalidCurrentSlotData),
    }
}

/// Get the maximum retry counter from `buffer`.
pub fn get_max_retry_count_from_buffer(buffer: &[u8]) -> Result<u8, Error> {
    super::is_valid_buffer(buffer, EXPECTED_LEN)?;
    // Information is stored in byte 5.
    let byte_5: u8 = buffer[5];
    Ok((MAX_RETRY_COUNT_MASK & byte_5) >> MAX_RETRY_COUNT_SHIFT)
}

// Set the rootfs `status` in `buffer` for a certain `slot`.
fn set_rootfs_status_in_buffer(buffer: &mut Vec<u8>, status: u8, slot: u8) -> Result<(), Error> {
    super::is_valid_buffer(&*buffer, EXPECTED_LEN)?;
    match slot {
        SLOT_A => {
            // Information is stored in byte 5 for Slot A.
            buffer[5] &= !ROOTFS_STATUS_A_MASK;
            buffer[5] += status << ROOTFS_STATUS_A_SHIFT;
        }
        SLOT_B => {
            // Information is stored in byte 4 for Slot B.
            buffer[4] &= !ROOTFS_STATUS_B_MASK;
            buffer[4] += status << ROOTFS_STATUS_B_SHIFT;
        }
        _ => return Err(Error::InvalidCurrentSlotData),
    }
    Ok(())
}

// Set the retry `counter` in `buffer` for a certain `slot`.
fn set_retry_count_in_buffer(buffer: &mut Vec<u8>, counter: u8, slot: u8) -> Result<(), Error> {
    super::is_valid_buffer(&*buffer, EXPECTED_LEN)?;
    // Information is stored in byte 4 for both Slots.
    match slot {
        SLOT_A => {
            buffer[4] &= !RETRY_COUNT_A_MASK;
            buffer[4] += counter << RETRY_COUNT_A_SHIFT;
        }
        SLOT_B => {
            buffer[4] &= !RETRY_COUNT_B_MASK;
            buffer[4] += counter << RETRY_COUNT_B_SHIFT;
        }
        _ => return Err(Error::InvalidCurrentSlotData),
    }
    Ok(())
}

/// Gets the raw current slot.
pub fn get_current_slot() -> Result<u8, Error> {
    let efivar = super::EfiVar::open(PATH, EXPECTED_LEN)?;
    get_active_slot_from_buffer(&efivar.buffer)
}

/// Get the raw rootfs status for a certain `slot`.
pub fn get_rootfs_status(slot: u8) -> Result<u8, Error> {
    let efivar = super::EfiVar::open(PATH, EXPECTED_LEN)?;
    get_rootfs_status_from_buffer(&efivar.buffer, slot)
}

/// Get the retry count for a certain `slot`.
pub fn get_retry_count(slot: u8) -> Result<u8, Error> {
    let efivar = super::EfiVar::open(PATH, EXPECTED_LEN)?;
    get_retry_count_from_buffer(&efivar.buffer, slot)
}

/// Get the maximum retry count.
pub fn get_max_retry_count() -> Result<u8, Error> {
    let efivar = super::EfiVar::open(PATH, EXPECTED_LEN)?;
    get_max_retry_count_from_buffer(&efivar.buffer)
}

/// Set raw rootfs `status` for a certain `slot`.
pub fn set_rootfs_status(status: u8, slot: u8) -> Result<(), Error> {
    let mut efivar = super::EfiVar::open(PATH, EXPECTED_LEN)?;
    set_rootfs_status_in_buffer(&mut efivar.buffer, status, slot)?;
    efivar.write()
}

/// Set the retry `counter` for a certain `slot`.
pub fn set_retry_count(counter: u8, slot: u8) -> Result<(), Error> {
    let mut efivar = super::EfiVar::open(PATH, EXPECTED_LEN)?;
    set_retry_count_in_buffer(&mut efivar.buffer, counter, slot)?;
    efivar.write()
}

#[cfg(test)]
mod tests {
    // Unit testing only buffer based operations.
    use eyre::Result;

    use super::*;

    const CURRENT_SLOT_A_DATA: [u8; 8] = [0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    const CURRENT_SLOT_B_DATA: [u8; 8] = [0x07, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00];

    fn assert_current_slot(slot: u8, data: [u8; 8]) -> Result<()> {
        let buffer = Vec::from(data);
        let read_slot = get_active_slot_from_buffer(&buffer)?;
        assert_eq!(read_slot, slot, "Read unexpected current slot");
        Ok(())
    }

    // Test reading a certain Slot from accordingly configured buffers.
    #[test]
    fn get_current_slot() -> Result<()> {
        assert_current_slot(SLOT_A, CURRENT_SLOT_A_DATA)?;
        assert_current_slot(SLOT_B, CURRENT_SLOT_B_DATA)?;
        Ok(())
    }

    const ROOTFS_STATUS_NORMAL_A_DATA: [u8; 8] = [0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    const ROOTFS_STATUS_UPD_IN_PROCESS_A_DATA: [u8; 8] =
        [0x07, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00];
    const ROOTFS_STATUS_UPD_DONE_A_DATA: [u8; 8] = [0x07, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00];
    const ROOTFS_STATUS_UNBOOTABLE_A_DATA: [u8; 8] =
        [0x07, 0x00, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00];
    const ROOTFS_STATUS_NORMAL_B_DATA: [u8; 8] = [0x07, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00];
    const ROOTFS_STATUS_UPD_IN_PROCESS_B_DATA: [u8; 8] =
        [0x07, 0x00, 0x00, 0x00, 0x42, 0x00, 0x00, 0x00];
    const ROOTFS_STATUS_UPD_DONE_B_DATA: [u8; 8] = [0x07, 0x00, 0x00, 0x00, 0x82, 0x00, 0x00, 0x00];
    const ROOTFS_STATUS_UNBOOTABLE_B_DATA: [u8; 8] =
        [0x07, 0x00, 0x00, 0x00, 0xc2, 0x00, 0x00, 0x00];

    fn assert_current_rootfs_status(status: u8, data: [u8; 8]) -> Result<()> {
        let buffer = Vec::from(data);
        let read_slot = get_active_slot_from_buffer(&buffer)?;
        let read_status = get_rootfs_status_from_buffer(&buffer, read_slot)?;
        assert_eq!(read_status, status, "Read unexpected current rootfs status");
        Ok(())
    }

    // Test reading certain rootfs status from accordingly configured buffers.
    #[test]
    fn get_current_rootfs_status() -> Result<()> {
        assert_current_rootfs_status(ROOTFS_STATUS_NORMAL, ROOTFS_STATUS_NORMAL_A_DATA)?;
        assert_current_rootfs_status(
            ROOTFS_STATUS_UPD_IN_PROCESS,
            ROOTFS_STATUS_UPD_IN_PROCESS_A_DATA,
        )?;
        assert_current_rootfs_status(ROOTFS_STATUS_UPD_DONE, ROOTFS_STATUS_UPD_DONE_A_DATA)?;
        assert_current_rootfs_status(ROOTFS_STATUS_UNBOOTABLE, ROOTFS_STATUS_UNBOOTABLE_A_DATA)?;
        assert_current_rootfs_status(ROOTFS_STATUS_NORMAL, ROOTFS_STATUS_NORMAL_B_DATA)?;
        assert_current_rootfs_status(
            ROOTFS_STATUS_UPD_IN_PROCESS,
            ROOTFS_STATUS_UPD_IN_PROCESS_B_DATA,
        )?;
        assert_current_rootfs_status(ROOTFS_STATUS_UPD_DONE, ROOTFS_STATUS_UPD_DONE_B_DATA)?;
        assert_current_rootfs_status(ROOTFS_STATUS_UNBOOTABLE, ROOTFS_STATUS_UNBOOTABLE_B_DATA)?;
        Ok(())
    }

    const CURRENT_SLOT_A_RETRY_0_DATA: [u8; 8] = [0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    const CURRENT_SLOT_A_RETRY_1_DATA: [u8; 8] = [0x07, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00, 0x00];
    const CURRENT_SLOT_A_RETRY_2_DATA: [u8; 8] = [0x07, 0x00, 0x00, 0x00, 0x20, 0x00, 0x00, 0x00];
    const CURRENT_SLOT_A_RETRY_3_DATA: [u8; 8] = [0x07, 0x00, 0x00, 0x00, 0x30, 0x00, 0x00, 0x00];
    const CURRENT_SLOT_B_RETRY_0_DATA: [u8; 8] = [0x07, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00];
    const CURRENT_SLOT_B_RETRY_1_DATA: [u8; 8] = [0x07, 0x00, 0x00, 0x00, 0x06, 0x00, 0x00, 0x00];
    const CURRENT_SLOT_B_RETRY_2_DATA: [u8; 8] = [0x07, 0x00, 0x00, 0x00, 0x0a, 0x00, 0x00, 0x00];
    const CURRENT_SLOT_B_RETRY_3_DATA: [u8; 8] = [0x07, 0x00, 0x00, 0x00, 0x0e, 0x00, 0x00, 0x00];

    fn assert_current_retry_count(count: u8, data: [u8; 8]) -> Result<()> {
        let buffer = Vec::from(data);
        let read_slot = get_active_slot_from_buffer(&buffer)?;
        let current_count = get_retry_count_from_buffer(&buffer, read_slot)?;
        assert_eq!(
            current_count, count,
            "Read unexpected current rootfs status"
        );
        Ok(())
    }

    // Test reading certain retry count from accordingly configured buffers.
    #[test]
    fn test_get_current_retry_count() -> Result<()> {
        assert_current_retry_count(0, CURRENT_SLOT_A_RETRY_0_DATA)?;
        assert_current_retry_count(1, CURRENT_SLOT_A_RETRY_1_DATA)?;
        assert_current_retry_count(2, CURRENT_SLOT_A_RETRY_2_DATA)?;
        assert_current_retry_count(3, CURRENT_SLOT_A_RETRY_3_DATA)?;
        assert_current_retry_count(0, CURRENT_SLOT_B_RETRY_0_DATA)?;
        assert_current_retry_count(1, CURRENT_SLOT_B_RETRY_1_DATA)?;
        assert_current_retry_count(2, CURRENT_SLOT_B_RETRY_2_DATA)?;
        assert_current_retry_count(3, CURRENT_SLOT_B_RETRY_3_DATA)?;
        Ok(())
    }

    const MAX_RETRY_COUNT_0_DATA: [u8; 8] = [0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    const MAX_RETRY_COUNT_1_DATA: [u8; 8] = [0x07, 0x00, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00];
    const MAX_RETRY_COUNT_2_DATA: [u8; 8] = [0x07, 0x00, 0x00, 0x00, 0x00, 0x80, 0x00, 0x00];
    const MAX_RETRY_COUNT_3_DATA: [u8; 8] = [0x07, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x00, 0x00];

    fn assert_max_retry_count(count: u8, data: [u8; 8]) -> Result<()> {
        let buffer = Vec::from(data);
        let max_count = get_max_retry_count_from_buffer(&buffer)?;
        assert_eq!(max_count, count, "Read unexpected current rootfs status");
        Ok(())
    }

    // Test reading certain max retry count from accordingly configured buffers.
    #[test]
    fn get_max_retry_count() -> Result<()> {
        assert_max_retry_count(0, MAX_RETRY_COUNT_0_DATA)?;
        assert_max_retry_count(1, MAX_RETRY_COUNT_1_DATA)?;
        assert_max_retry_count(2, MAX_RETRY_COUNT_2_DATA)?;
        assert_max_retry_count(3, MAX_RETRY_COUNT_3_DATA)?;
        Ok(())
    }

    #[test]
    fn test_set_current_rootfs_status() -> Result<()> {
        let test_data_slot_a = [
            (ROOTFS_STATUS_NORMAL, ROOTFS_STATUS_NORMAL_A_DATA),
            (
                ROOTFS_STATUS_UPD_IN_PROCESS,
                ROOTFS_STATUS_UPD_IN_PROCESS_A_DATA,
            ),
            (ROOTFS_STATUS_UPD_DONE, ROOTFS_STATUS_UPD_DONE_A_DATA),
            (ROOTFS_STATUS_UNBOOTABLE, ROOTFS_STATUS_UNBOOTABLE_A_DATA),
        ];
        for original_data in 0..3_usize {
            for new_status in 0..3_usize {
                let mut buffer = Vec::from(test_data_slot_a[original_data].1);
                set_rootfs_status_in_buffer(&mut buffer, test_data_slot_a[new_status].0, SLOT_A)?;
                let data_status = get_rootfs_status_from_buffer(&buffer, SLOT_A)?;
                assert_eq!(
                    new_status as u8, data_status,
                    "Retry count unexpected after set"
                );
            }
        }
        let test_data_slot_b = [
            (ROOTFS_STATUS_NORMAL, ROOTFS_STATUS_NORMAL_B_DATA),
            (
                ROOTFS_STATUS_UPD_IN_PROCESS,
                ROOTFS_STATUS_UPD_IN_PROCESS_B_DATA,
            ),
            (ROOTFS_STATUS_UPD_DONE, ROOTFS_STATUS_UPD_DONE_B_DATA),
            (ROOTFS_STATUS_UNBOOTABLE, ROOTFS_STATUS_UNBOOTABLE_B_DATA),
        ];
        for original_data in 0..3_usize {
            for new_status in 0..3_usize {
                let mut buffer = Vec::from(test_data_slot_b[original_data].1);
                set_rootfs_status_in_buffer(&mut buffer, test_data_slot_b[new_status].0, SLOT_B)?;
                let data_status = get_rootfs_status_from_buffer(&buffer, SLOT_B)?;
                assert_eq!(
                    new_status as u8, data_status,
                    "Retry count unexpected after set"
                );
            }
        }
        Ok(())
    }

    // Test
    #[test]
    fn test_set_retry_count() -> Result<()> {
        let test_data_slot_a = [
            CURRENT_SLOT_A_RETRY_0_DATA,
            CURRENT_SLOT_A_RETRY_1_DATA,
            CURRENT_SLOT_A_RETRY_2_DATA,
            CURRENT_SLOT_A_RETRY_3_DATA,
        ];
        for original_retry in 0..3_usize {
            for new_retry in 0..3_u8 {
                let mut buffer = Vec::from(test_data_slot_a[original_retry]);
                set_retry_count_in_buffer(&mut buffer, new_retry, SLOT_A)?;
                let data_counter = get_retry_count_from_buffer(&buffer, SLOT_A)?;
                assert_eq!(new_retry, data_counter, "Retry count unexpected after set");
            }
        }
        let test_data_slot_b = [
            CURRENT_SLOT_B_RETRY_0_DATA,
            CURRENT_SLOT_B_RETRY_1_DATA,
            CURRENT_SLOT_B_RETRY_2_DATA,
            CURRENT_SLOT_B_RETRY_3_DATA,
        ];
        for original_retry in 0..3_usize {
            for new_retry in 0..3_u8 {
                let mut buffer = Vec::from(test_data_slot_b[original_retry]);
                set_retry_count_in_buffer(&mut buffer, new_retry, SLOT_B)?;
                let data_counter = get_retry_count_from_buffer(&buffer, SLOT_B)?;
                assert_eq!(new_retry, data_counter, "Retry count unexpected after set");
            }
        }
        Ok(())
    }
}
