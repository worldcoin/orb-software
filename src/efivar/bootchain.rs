//! `BootChainOsOverride` efivar.
//!
//! Example value for this efivar: [0x07, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]
//!
//! Byte 4 (numbering from zero) represents the current configuration for the next active boot slot.
//!

#[cfg(test)]
use super::{SLOT_A, SLOT_B};
use crate::Error;

#[cfg(not(test))]
pub const PATH: &str =
    "/sys/firmware/efi/efivars/BootChainOsOverride-781e084c-a330-417c-b678-38e696380cb9";
#[cfg(test)]
pub const PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/testbootchainefivar");

const EXPECTED_LEN: usize = 8;

// Get the raw current boot slot from a buffer.
fn get_next_boot_slot_from_buffer(buffer: &[u8]) -> Result<u8, Error> {
    super::is_valid_buffer(buffer, EXPECTED_LEN)?;
    // Next active boot slot information can be found in byte 4.
    // 0 == slot A
    // 1 == slot B
    // 255 == default (next rootfs boot slot is chosen from the active bootloader slot)
    Ok(buffer[4])
}

// Set the next boot slot in given buffer.
fn set_next_boot_slot_in_buffer(buffer: &mut Vec<u8>, slot: u8) -> Result<(), Error> {
    super::is_valid_buffer(&*buffer, EXPECTED_LEN)?;
    // Next active boot slot information can be found in byte 4.
    buffer[4] = slot;
    Ok(())
}

/// Gets the raw current boot slot.
pub fn get_next_boot_slot() -> Result<u8, Error> {
    let efivar = super::EfiVar::open(PATH, EXPECTED_LEN)?;
    get_next_boot_slot_from_buffer(&efivar.buffer)
}

/// Set the next boot slot.
pub fn set_next_boot_slot(slot: u8) -> Result<(), Error> {
    let mut efivar = super::EfiVar::open(PATH, EXPECTED_LEN)?;
    set_next_boot_slot_in_buffer(&mut efivar.buffer, slot)?;
    efivar.write()
}

#[cfg(test)]
mod tests {
    // Unit testing only buffer based operations.
    use eyre::Result;

    use super::*;

    const EFIVAR_BUFFER_BOOT_SLOT_A: [u8; 8] = [0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    const EFIVAR_BUFFER_BOOT_SLOT_B: [u8; 8] = [0x07, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00];

    #[test]
    fn test_get_next_boot_slot() -> Result<()> {
        // Read Slot A from configured Slot A.
        let buffer = Vec::from(EFIVAR_BUFFER_BOOT_SLOT_A);
        let slot = get_next_boot_slot_from_buffer(&buffer)?;
        assert_eq!(slot, SLOT_A, "Read unexpected next boot slot");

        // Read Slot B from configured Slot B.
        let buffer = Vec::from(EFIVAR_BUFFER_BOOT_SLOT_B);
        let slot = get_next_boot_slot_from_buffer(&buffer)?;
        assert_eq!(slot, SLOT_B, "Read unexpected next boot slot");
        Ok(())
    }

    #[test]
    fn test_set_next_boot_slot() -> Result<()> {
        // Set Slot A again on already configured Slot A.
        let mut buffer = Vec::from(EFIVAR_BUFFER_BOOT_SLOT_A);
        set_next_boot_slot_in_buffer(&mut buffer, SLOT_A)?;
        assert_eq!(
            buffer, EFIVAR_BUFFER_BOOT_SLOT_A,
            "Buffer was changed unexpectedly"
        );

        // Set Slot B again on already configured Slot B.
        let mut buffer = Vec::from(EFIVAR_BUFFER_BOOT_SLOT_B);
        set_next_boot_slot_in_buffer(&mut buffer, SLOT_B)?;
        assert_eq!(
            buffer, EFIVAR_BUFFER_BOOT_SLOT_B,
            "Buffer was changed unexpectedly"
        );

        // Set Slot B on configured Slot A.
        let mut buffer = Vec::from(EFIVAR_BUFFER_BOOT_SLOT_A);
        set_next_boot_slot_in_buffer(&mut buffer, SLOT_B)?;
        assert_eq!(
            buffer, EFIVAR_BUFFER_BOOT_SLOT_B,
            "Buffer wasn't changed accordingly"
        );

        // Set Slot A on configured Slot B.
        let mut buffer = Vec::from(EFIVAR_BUFFER_BOOT_SLOT_B);
        set_next_boot_slot_in_buffer(&mut buffer, SLOT_A)?;
        assert_eq!(
            buffer, EFIVAR_BUFFER_BOOT_SLOT_A,
            "Buffer wasn't changed accordingly"
        );
        Ok(())
    }
}
