use std::{fs::File, io, path::Path};

use eyre::{ensure, WrapErr as _};
use gpt::disk::LogicalBlockSize::Lb512;
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
            .get_disk()
            .wrap_err("failed to open target GPT device")?;
        let part = gpt_component.get_partition(&disk, active_slot)?;
        let disk = disk.take_device();

        let part_len = part.bytes_len(gpt::disk::LogicalBlockSize::Lb512)?;
        let mut disk_part = gpt::partition::TakePartition::take(
            part,
            disk,
            gpt::disk::LogicalBlockSize::Lb512,
            part_len,
        );

        gpt_component.update(target_slot, &mut disk_part)?;
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
    fn update<R>(&self, slot: Slot, mut src: &mut R) -> eyre::Result<()>
    where
        R: io::Read + io::Seek + ?Sized,
    {
        DATADOG
            .incr("orb.update.count.component.gpt", ["status:started"])
            .or_log();
        let disk = self.get_disk().expect("failed to open target GPT device");
        let part = self.get_partition(&disk, slot)?;

        if !self.is_redundant() {
            match unmount_partition_by_label(&self.label) {
                Ok(_) => debug!("partition unmounted successfully"),
                Err(err) => {
                    warn!("failed to unmount partition; continuing anyway: {err:?}")
                }
            }
        }

        src.seek(std::io::SeekFrom::Start(0))
            .wrap_err("failed to seek to start of GPT update source")?;
        let src_len = src
            .seek(std::io::SeekFrom::End(0))
            .wrap_err("failed to seek to end of GPT update source")?;
        src.seek(std::io::SeekFrom::Start(0))
            .expect("couldn't re-seek to start of GPT update source!");

        debug!("-- preparing to write {:?} bytes", src_len);

        let part_len = part.bytes_len(Lb512)?;
        ensure!(
            src_len <= part_len,
            "partition {} is too small to write component of size {:?}",
            part_len,
            src_len,
        );

        let mut disk = disk.take_device();
        disk.seek(std::io::SeekFrom::Start(
            part.bytes_start(Lb512).wrap_err_with(|| {
                format!(
                    "failed to get GPT partition offset for partition `{}` (assuming 512-byte LB)",
                    self.label
                )
            })?,
        ))
        .wrap_err_with(|| {
            format!(
                "failed to seek to partition offset for partition `{}` in device `{}`",
                self.label, self.device
            )
        })?;
        debug!("-- seeking up to offset {:?}", part.bytes_start(Lb512)?);

        // TODO: Possibly use .by_ref().take(src_len)
        std::io::copy(&mut src, &mut disk)
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

        disk.flush()
            .wrap_err_with(|| format!("GPT disk `{}` flush failed", self.device))?;
        debug!("-- flushed!");

        DATADOG
            .incr("orb.update.count.component.gpt", ["status:write_complete"])
            .or_log();
        Ok(())
    }
}
