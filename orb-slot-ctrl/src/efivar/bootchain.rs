//! `BootChain` efivars.
//!
//! * `BootChainFwCurrent` - represents the current boot slot (readonly)
//! * `BootChainFwNext` - represents the next boot slot
//!
//! Bits of interest are found in byte 4 for all efivars.

use super::{is_valid_buffer, EfiVar, SLOT_A, SLOT_B};
use crate::Error;

pub const PATH_CURRENT: &str =
    "BootChainFwCurrent-781e084c-a330-417c-b678-38e696380cb9";
pub const PATH_NEXT: &str = "BootChainFwNext-781e084c-a330-417c-b678-38e696380cb9";

const EXPECTED_LEN: usize = 8;
const NEXT_BOOT_SLOT_NEW_BUFFER: [u8; 8] =
    [0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];

/// Throws an `Error` if the given slot is invalid.
fn is_valid_slot(slot: u8) -> Result<(), Error> {
    match slot {
        SLOT_A | SLOT_B => Ok(()),
        _ => Err(Error::InvalidSlotData),
    }
}

// Get the slot from a buffer.
fn get_slot_from_buffer(buffer: &[u8]) -> Result<u8, Error> {
    is_valid_buffer(buffer, EXPECTED_LEN)?;
    Ok(buffer[4])
}

// Set the slot in given buffer.
fn set_slot_in_buffer(buffer: &mut Vec<u8>, slot: u8) -> Result<(), Error> {
    is_valid_buffer(&*buffer, EXPECTED_LEN)?;
    // Next boot slot information can be found in byte 4.
    buffer[4] = slot;
    Ok(())
}

/// Gets the raw current boot slot.
pub fn get_current_boot_slot() -> Result<u8, Error> {
    let efivar = EfiVar::from_path(PATH_CURRENT)?.read_fixed_len(EXPECTED_LEN)?;
    get_slot_from_buffer(&efivar)
}

/// Gets the raw next boot slot.
pub fn get_next_boot_slot() -> Result<u8, Error> {
    match EfiVar::from_path(PATH_NEXT)?.read_fixed_len(EXPECTED_LEN) {
        Ok(efivar) => Ok(get_slot_from_buffer(&efivar)?),
        Err(Error::OpenFile { path: _, source: _ }) => {
            // in this case the efivar does not exist yet because it gets created on demand and
            // the next boot slot will be the same as the current
            get_current_boot_slot()
        }
        Err(err) => Err(err),
    }
}

/// Set the next boot slot.
pub fn set_next_boot_slot(slot: u8) -> Result<(), Error> {
    is_valid_slot(slot)?;
    let efivar = super::EfiVar::from_path(PATH_NEXT)?;
    match efivar.read_fixed_len(EXPECTED_LEN) {
        Ok(mut val) => {
            set_slot_in_buffer(&mut val, slot)?;
            efivar.write(&val)
        }
        Err(Error::OpenFile { path: _, source: _ }) => {
            // in this case the efivar does not exist yet because and needs to be created.
            let mut buffer = Vec::from(NEXT_BOOT_SLOT_NEW_BUFFER);
            set_slot_in_buffer(&mut buffer, slot)?;
            efivar.create_and_write(&buffer)
        }
        Err(err) => Err(err),
    }
}

#[cfg(test)]
mod tests {
    // Unit testing only buffer based operations.
    use eyre::Result;

    use super::*;

    const EFIVAR_BUFFER_BOOT_SLOT_A: [u8; 8] =
        [0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    const EFIVAR_BUFFER_BOOT_SLOT_B: [u8; 8] =
        [0x07, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00];

    #[test]
    fn test_get_slot_from_buffer() -> Result<()> {
        // Read Slot A from configured Slot A.
        let buffer = Vec::from(EFIVAR_BUFFER_BOOT_SLOT_A);
        let slot = get_slot_from_buffer(&buffer)?;
        assert_eq!(slot, SLOT_A, "Read unexpected next boot slot");

        // Read Slot B from configured Slot B.
        let buffer = Vec::from(EFIVAR_BUFFER_BOOT_SLOT_B);
        let slot = get_slot_from_buffer(&buffer)?;
        assert_eq!(slot, SLOT_B, "Read unexpected next boot slot");
        Ok(())
    }

    #[test]
    fn test_set_slot_in_buffer() -> Result<()> {
        // Set Slot A again on already configured Slot A.
        let mut buffer = Vec::from(EFIVAR_BUFFER_BOOT_SLOT_A);
        set_slot_in_buffer(&mut buffer, SLOT_A)?;
        assert_eq!(
            buffer, EFIVAR_BUFFER_BOOT_SLOT_A,
            "Buffer was changed unexpectedly"
        );

        // Set Slot B again on already configured Slot B.
        let mut buffer = Vec::from(EFIVAR_BUFFER_BOOT_SLOT_B);
        set_slot_in_buffer(&mut buffer, SLOT_B)?;
        assert_eq!(
            buffer, EFIVAR_BUFFER_BOOT_SLOT_B,
            "Buffer was changed unexpectedly"
        );

        // Set Slot B on configured Slot A.
        let mut buffer = Vec::from(EFIVAR_BUFFER_BOOT_SLOT_A);
        set_slot_in_buffer(&mut buffer, SLOT_B)?;
        assert_eq!(
            buffer, EFIVAR_BUFFER_BOOT_SLOT_B,
            "Buffer wasn't changed accordingly"
        );

        // Set Slot A on configured Slot B.
        let mut buffer = Vec::from(EFIVAR_BUFFER_BOOT_SLOT_B);
        set_slot_in_buffer(&mut buffer, SLOT_A)?;
        assert_eq!(
            buffer, EFIVAR_BUFFER_BOOT_SLOT_A,
            "Buffer wasn't changed accordingly"
        );
        Ok(())
    }
}
