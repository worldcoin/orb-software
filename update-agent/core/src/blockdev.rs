use std::path::PathBuf;

use gpt::partition_types;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed to find EFI System Partition")]
    ESPPartitionNotFound,
    #[error("multiple EFI System Partitions found: {0:?}")]
    MultipleESPPartitions(Vec<(PathBuf, u32)>),
}

fn find_efi_partition() -> Result<(PathBuf, u32), Error> {
    let mut efi_partitions = Vec::new();

    // The device either uses -> emmc/sd card or an sd express card
    // for the primary storage. Iterating over the possible cases
    // to find the esp partition.
    for disk_path in ["/dev/mmcblk0", "/dev/mmcblk1", "/dev/nvme0n1"] {
        let Ok(gpt) = gpt::GptConfig::new().open(disk_path) else {
            continue;
        };

        for (partition_id, partition) in gpt.partitions().iter() {
            if partition.part_type_guid == partition_types::EFI {
                efi_partitions.push((PathBuf::from(disk_path), *partition_id));
            }
        }
    }

    match efi_partitions.len() {
        0 => Err(Error::ESPPartitionNotFound),
        1 => Ok(efi_partitions[0].clone()),
        _ => Err(Error::MultipleESPPartitions(efi_partitions)),
    }
}

// Derives the blockdevice referred to by the claim from
// the parent device of the esp partition.
// Some diamond devices use sd cards (mmcblk) while others
// use sd-express cards (mmcblk). All pearl devices use
// emmc (mmcblk)
pub fn find_root_blockdevice() -> Result<PathBuf, Error> {
    find_efi_partition().map(|(disk_path, _)| disk_path)
}

pub fn find_esp_partition() -> Result<PathBuf, Error> {
    let (disk_path, partition_id) = find_efi_partition()?;
    Ok(PathBuf::from(format!(
        "{}p{partition_id}",
        disk_path.display()
    )))
}
