use gpt::partition_types;
/// Nvidia docs: https://web.archive.org/web/20231012155023/https://docs.nvidia.com/jetson/archives/r35.3.1/DeveloperGuide/text/SD/Bootloader/UpdateAndRedundancy.html#manually-trigger-the-capsule-update
///
/// For UEFI documentation see: [1] 8.5.5 Delivery of Capsules via file on Mass Storage device
/// [1] https://uefi.org/sites/default/files/resources/UEFI_Spec_2_9_2021_03_18.pdf
use std::io;
use std::path::PathBuf;

use efivar::{EfiVarData, EfiVarDb};
use orb_update_agent_core::{components, Slot};
use thiserror::Error;

use super::Update;
use crate::mount::TemporaryMount;

// For values see
pub const EFI_OS_INDICATIONS: &str =
    "OsIndications-8be4df61-93ca-11d2-aa0d-00e098032b8c";
pub const EFI_OS_REQUEST_CAPSULE_UPDATE: [u8; 12] =
    [0x07, 0x0, 0x0, 0x0, 0x04, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0];

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
    WriteOsIndications(#[source] eyre::Report),
    #[error("Failed to find EFI System Partition")]
    ESPPartitionNotFound,
    #[error("Failed to open GPT disk {0}: {1}")]
    OpenGptDisk(PathBuf, #[source] gpt::GptError),
    #[error("Multiple EFI system partitions found on {0}: {1:?}")]
    MultipleESPPartitions(PathBuf, Vec<u32>),
}

fn find_esp_partition() -> Result<PathBuf, Error> {
    // Try common storage devices in order
    let devices = ["/dev/nvme0n1", "/dev/mmcblk0"];

    for device_path in &devices {
        // Try to open the device as a GPT disk
        let disk = match gpt::GptConfig::new().open(device_path) {
            Ok(disk) => disk,
            Err(gpt::GptError::Io(io_error))
                if io_error.kind() == io::ErrorKind::NotFound =>
            {
                // Device doesn't exist (ENOENT), skip to next device
                continue;
            }
            Err(e) => return Err(Error::OpenGptDisk(PathBuf::from(device_path), e)),
        };

        // Find all EFI System Partitions
        let efi_partitions: Vec<u32> = disk
            .partitions()
            .iter()
            .filter_map(|(partition_id, partition)| {
                if partition.part_type_guid == partition_types::EFI {
                    Some(*partition_id)
                } else {
                    None
                }
            })
            .collect();

        match efi_partitions.len() {
            0 => continue, // No EFI partition on this device, try next
            1 => {
                return Ok(PathBuf::from(format!(
                    "{}p{}",
                    device_path, efi_partitions[0]
                )))
            }
            _ => {
                return Err(Error::MultipleESPPartitions(
                    PathBuf::from(device_path),
                    efi_partitions,
                ))
            }
        }
    }

    Err(Error::ESPPartitionNotFound)
}

fn save_capsule<R>(mut src: R) -> Result<(), Error>
where
    R: io::Read + io::Seek,
{
    let esp_partition_path = find_esp_partition()?;
    let esp = TemporaryMount::new(&esp_partition_path)
        .map_err(|e| Error::Mount(e, esp_partition_path))?;
    let mut capsule = esp
        .create_file(CAPSULE_INSTALL_NAME)
        .map_err(|e| Error::CreateFile(e, CAPSULE_INSTALL_NAME.into()))?;
    io::copy(&mut src, &mut capsule).map_err(Error::CopyCapsule)?;
    Ok(())
}

impl Update for components::Capsule {
    // TODO EFI can't update any arbitrary slot, only the *other* slot. So we
    // don't check which slot we're updating. Maybe check that the slot is the
    // right one?
    fn update<R>(&self, _: Slot, src: R) -> eyre::Result<()>
    where
        R: io::Read + io::Seek,
    {
        save_capsule(src)?;

        EfiVarDb::from_rootfs("/")
            .and_then(|db| db.get_var(EFI_OS_INDICATIONS))
            .and_then(|var| {
                var.write(&EfiVarData::new(
                    EFI_OS_REQUEST_CAPSULE_UPDATE[0],
                    &EFI_OS_REQUEST_CAPSULE_UPDATE[4..],
                ))
            })
            .map_err(Error::WriteOsIndications)?;

        Ok(())
    }
}
