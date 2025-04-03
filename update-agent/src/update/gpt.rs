use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom, Write},
    path::Path,
};

use eyre::{ensure, WrapErr as _};
use gpt::disk::LogicalBlockSize::Lb512;
use orb_io_utils::ClampedSeek;
use orb_update_agent_core::{
    components,
    telemetry::{LogOnError, DATADOG},
    Claim, Component, Components, Slot, VersionMap,
};
use tracing::{debug, warn};

use super::Update;
use crate::{
    component::Component as RuntimeComponent, mount::unmount_partition_by_label,
};

// Find all redundant GPT components that are listed in `system_components` buit
// which were not updated as part of the base update
// specified in the update manifest
fn find_not_updated_redundant_gpt_components<'a: 'c, 'b: 'c, 'c>(
    system_components: &'a Components,
    update_components: &'b [RuntimeComponent],
) -> impl Iterator<Item = (&'a String, &'a components::Gpt)> + 'c {
    system_components
        .iter()
        .filter_map(move |(name, component)| match component {
            Component::Gpt(gpt)
                if gpt.is_redundant()
                    && !update_components.iter().any(|rc| rc.name() == name) =>
            {
                Some((name, gpt))
            }
            _ => None,
        })
}

/// After confirming reads work at the extremeties of the given range, this function
/// will seek to `range.start`.
fn confirm_read_works_at_bounds(
    mut f: impl Read + Seek,
    range: std::ops::Range<u64>,
) -> eyre::Result<()> {
    let len = f
        .seek(SeekFrom::End(0))
        .wrap_err("failed to seek to End(0)")?;
    ensure!(
        range.end <= len,
        "range end {} was out of bounds of seek length {}",
        range.end,
        len
    );

    f.seek(SeekFrom::Start(range.start))
        .wrap_err_with(|| format!("failed to seek to `range.start` {}", range.start))?;
    f.read_exact(&mut [0; 1])
        .wrap_err_with(|| format!("failed to read at `range.start` {}", range.start))?;
    f.seek(SeekFrom::Start(range.end - 1)).wrap_err_with(|| {
        format!("failed to seek to `range.end-1` {}", range.end - 1)
    })?;
    f.read_exact(&mut [0; 1]).wrap_err_with(|| {
        format!("failed to read at `range.end-1` {}", range.end - 1)
    })?;
    f.seek(SeekFrom::Start(range.start)).wrap_err_with(|| {
        format!("failed to return to `range.start` {}", range.start)
    })?;

    Ok(())
}

/// Update all redundant GPT components that were not explicitly updated as part of the manifest
/// by copying them from the currently active.
pub fn copy_not_updated_redundant_components(
    claim: &Claim,
    update_components: &[RuntimeComponent],
    active_slot: Slot,
    version_map: &mut VersionMap,
    version_map_dst: &Path,
) -> eyre::Result<()> {
    let target_slot = active_slot.opposite();
    for (name, gpt_component) in find_not_updated_redundant_gpt_components(
        claim.system_components(),
        update_components,
    ) {
        let disk = gpt_component
            .get_disk(false)
            .wrap_err("failed to open target GPT device")?;
        let partition_entry = gpt_component.read_partition_entry(&disk, active_slot)?;

        // Clamp disk to be just the partition's range
        let mut disk_partition_reader = {
            let mut disk_file: File = disk.take_device();

            let part_len =
                partition_entry.bytes_len(gpt::disk::LogicalBlockSize::Lb512)?;
            let part_start =
                partition_entry.bytes_start(gpt::disk::LogicalBlockSize::Lb512)?;

            confirm_read_works_at_bounds(
                &mut disk_file,
                part_start..(part_start + part_len),
            )
            .wrap_err("failed to confirm disk partition reads at bounds")?;

            ClampedSeek::new(disk_file, ..part_len)?
        };

        gpt_component.update(target_slot, &mut disk_partition_reader)?;
        if !version_map.mirror_redundant_component_version(name, target_slot.opposite())
        {
            warn!("gpt_component `{name}` is either missing from source group or not redundant");
        }

        serde_json::to_writer(
            &File::options()
                .write(true)
                .read(true)
                .truncate(true)
                .open(version_map_dst)?,
            &version_map,
        )
        .wrap_err("saving to versions file failed")?;
    }
    Ok(())
}

impl Update for components::Gpt {
    fn update<R>(&self, slot: Slot, mut src: R) -> eyre::Result<()>
    where
        R: io::Read + io::Seek,
    {
        let src_len = src
            .seek(SeekFrom::End(0))
            .wrap_err("failed to get length of GPT update source")?;
        if src_len == 0 {
            warn!("source length was 0, skipping"); // TODO: is this ever possible?
            return Ok(());
        }

        DATADOG
            .incr("orb.update.count.component.gpt", ["status:started"])
            .or_log();

        let disk = self
            .get_disk(true)
            .expect("failed to open target GPT device");
        let part_entry = self.read_partition_entry(&disk, slot)?;

        if !self.is_redundant() {
            match unmount_partition_by_label(&self.label) {
                Ok(_) => debug!("partition unmounted successfully"),
                Err(err) => {
                    warn!("failed to unmount partition; continuing anyway: {err:?}")
                }
            }
        }

        confirm_read_works_at_bounds(&mut src, 0..src_len)
            .wrap_err("failed to confirm source reads at bounds")?;

        debug!("-- preparing to write {:?} bytes", src_len);

        let part_len = part_entry.bytes_len(Lb512)?;
        ensure!(
            src_len <= part_len,
            "partition {} with len {} is too small to receive component of size {:?}",
            self.label,
            part_len,
            src_len,
        );

        let mut partition_file = {
            let mut disk_file = disk.take_device();
            let part_start = part_entry.bytes_start(Lb512).wrap_err_with(|| {
                format!(
                    "failed to get GPT partition offset for partition \
                    `{}` (assuming 512-byte LB)",
                    self.label
                )
            })?;

            debug!("-- seeking up to offset {:?}", part_start);
            disk_file
                .seek(SeekFrom::Start(part_start))
                .wrap_err_with(|| {
                    format!(
                        "failed to seek to partition offset for partition \
                        `{}` in device `{}`",
                        self.label, self.device
                    )
                })?;

            disk_file
        };

        std::io::copy(&mut src.take(src_len), &mut partition_file)
            .wrap_err_with(|| {
                format!(
                    "I/O copy failed for GPT update from source to partition `{}`",
                    self.label
                )
            })
            .map_err({
                DATADOG
                    .incr("orb.update.count.component.gpt", ["status:write_error"])
                    .or_log();
                |e| e
            })?;
        debug!("-- copied!");

        partition_file
            .flush()
            .and_then(|()| partition_file.sync_all())
            .wrap_err_with(|| format!("GPT disk `{}` flush failed", self.device))?;
        debug!("-- flushed!");

        DATADOG
            .incr("orb.update.count.component.gpt", ["status:write_complete"])
            .or_log();
        Ok(())
    }
}
