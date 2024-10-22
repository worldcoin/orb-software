/// Nvidia docs: https://web.archive.org/web/20231012155023/https://docs.nvidia.com/jetson/archives/r35.3.1/DeveloperGuide/text/SD/Bootloader/UpdateAndRedundancy.html#manually-trigger-the-capsule-update
///
/// For UEFI documentation see: [1] 8.5.5 Delivery of Capsules via file on Mass Storage device
/// [1] https://uefi.org/sites/default/files/resources/UEFI_Spec_2_9_2021_03_18.pdf
use std::io;
use std::path::PathBuf;

use orb_update_agent_core::{components, Slot};
use slot_ctrl::EfiVar;
use thiserror::Error;

use super::Update;
use crate::mount::TemporaryMount;

// For values see
pub const EFI_OS_INDICATIONS: &str =
    "/sys/firmware/efi/efivars/OsIndications-8be4df61-93ca-11d2-aa0d-00e098032b8c";
pub const EFI_OS_REQUEST_CAPSULE_UPDATE: [u8; 12] =
    [7, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0];

const ESP_PARTITION_PATH: &str = "/dev/disk/by-partlabel/esp";
const CAPSULE_INSTALL_NAME: &str = "EFI/UpdateCapsule/bootloader-update.Cap";

#[derive(Debug, Error)]
enum Error {
    #[error("Failed to mount {1}: {0}")]
    Mount(#[source] std::io::Error, PathBuf),
    #[error("Failed to create file {1}: {0}")]
    CreateFile(#[source] std::io::Error, PathBuf),
    #[error("Failed to copy capsule: {0}")]
    CopyCapsule(#[source] std::io::Error),
    #[error("Failed to write OsIndications: {0}")]
    WriteOsIndications(#[source] slot_ctrl::Error),
}

fn save_capsule<R>(src: &mut R) -> Result<(), Error>
where
    R: io::Read + io::Seek + ?Sized,
{
    let esp = TemporaryMount::new(ESP_PARTITION_PATH)
        .map_err(|e| Error::Mount(e, ESP_PARTITION_PATH.into()))?;
    let mut capsule = esp
        .create_file(CAPSULE_INSTALL_NAME)
        .map_err(|e| Error::CreateFile(e, CAPSULE_INSTALL_NAME.into()))?;
    io::copy(src, &mut capsule).map_err(Error::CopyCapsule)?;
    Ok(())
}

impl Update for components::Capsule {
    // TODO EFI can't update any arbitrary slot, only the *other* slot. So we
    // don't check which slot we're updating. Maybe check that the slot is the
    // right one?
    fn update<R>(&self, _: Slot, src: &mut R) -> eyre::Result<()>
    where
        R: io::Read + io::Seek + ?Sized,
    {
        save_capsule(src)?;
        let efivar =
            EfiVar::from_path(EFI_OS_INDICATIONS).map_err(Error::WriteOsIndications)?;
        efivar
            .create_and_write(&EFI_OS_REQUEST_CAPSULE_UPDATE)
            .map_err(Error::WriteOsIndications)?;
        Ok(())
    }
}
