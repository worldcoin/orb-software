use eyre::{ensure, eyre, WrapErr as _};
use orb_dogd::MetricEmitter;
use orb_update_agent_core::{components, Slot};
use std::io::{self, Seek as _, Write};
use tracing::debug;

use super::Update;

const METRIC_NAME: &str = "orb.platform.update.component.raw";

/// Resolves the byte offset at which `src_len` bytes may be written to a block device of
/// `block_dev_len` bytes, or errors if that write would not fit.
///
/// `size` and `offset` come from the claim's `system_components`, so this arithmetic
/// must not be allowed to wrap: no build profile enables overflow checks, and a wrapped
/// sum turns the bounds check into a no-op that permits a write past the end of the
/// device. Kept free of any device handle so it can be unit tested off-target.
fn checked_write_bounds(
    size: u64,
    offset: u64,
    redundant: bool,
    slot: Slot,
    src_len: u64,
    block_dev_len: u64,
) -> eyre::Result<u64> {
    let offset = if slot == Slot::B && redundant {
        size.checked_add(offset).ok_or_else(|| {
            eyre!(
                "redundant slot B offset overflows u64: size {size} + offset {offset}"
            )
        })?
    } else {
        offset
    };

    let write_end = src_len.checked_add(offset).ok_or_else(|| {
        eyre!("write range overflows u64: source length {src_len} + offset {offset}")
    })?;
    ensure!(
        block_dev_len >= write_end,
        "block device is too small to write {src_len} bytes starting at offset {offset}: \
         device length is {block_dev_len}",
    );

    Ok(offset)
}

impl Update for components::Raw {
    fn update<R, M>(&self, slot: Slot, mut src: R, metrics: &M) -> eyre::Result<()>
    where
        R: io::Read + io::Seek,
        M: MetricEmitter,
    {
        let _ = metrics
            .incr(METRIC_NAME, ["status:started"])
            .inspect_err(|e| tracing::error!("metric emit failed: {e:#?}"));
        let mut block_dev =
            self.get_file().wrap_err("failed to open target raw file")?;

        debug!("-- calculating source length");

        src.seek(std::io::SeekFrom::Start(0))?;
        let src_len = src.seek(std::io::SeekFrom::End(0))?;
        src.seek(std::io::SeekFrom::Start(0))?;

        debug!("-- updating with source length {:?}", src_len);
        debug!("-- calculating device length");

        block_dev
            .seek(std::io::SeekFrom::Start(0))
            .wrap_err("failed to seek to start of raw update source")?;
        let block_dev_len = block_dev
            .seek(std::io::SeekFrom::End(0))
            .wrap_err("failed to seek to end of raw update source")?;
        block_dev
            .seek(std::io::SeekFrom::Start(0))
            .expect("couldn't re-seek to start of raw update source!");

        debug!("-- updating with device length {:?}", block_dev_len);

        let offset = checked_write_bounds(
            self.size,
            self.offset,
            self.is_redundant(),
            slot,
            src_len,
            block_dev_len,
        )?;
        debug!("-- setting up offset to be {:?}", offset);
        debug!("-- device passed length check");

        block_dev
            .seek(std::io::SeekFrom::Start(offset))
            .wrap_err_with(|| {
                format!(
                    "failed to seek to partition offset `{offset}` for block device `{}`",
                    self.device
                )
            })?;
        debug!("-- seeking up to offset {:?}", offset);

        std::io::copy(&mut src, &mut block_dev)
            .wrap_err_with(|| {
                format!(
                    "I/O copy failed for raw update from source to block device `{}`",
                    self.device
                )
            })
            .inspect_err(|_| {
                let _ = metrics
                    .incr(METRIC_NAME, ["status:write_error"])
                    .inspect_err(|me| tracing::error!("metric emit failed: {me:#?}"));
            })?;
        debug!("-- copied!");

        block_dev
            .flush()
            .wrap_err_with(|| format!("block device `{}` flush failed", self.device))?;
        debug!("-- flushed!");

        let _ = metrics
            .incr(METRIC_NAME, ["status:write_complete"])
            .inspect_err(|e| tracing::error!("metric emit failed: {e:#?}"));

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_redundant_component_writes_at_its_offset() {
        for slot in [Slot::A, Slot::B] {
            assert_eq!(
                checked_write_bounds(100, 1000, false, slot, 50, 2000).unwrap(),
                1000
            );
        }
    }

    #[test]
    fn redundant_component_writes_slot_b_after_slot_a() {
        assert_eq!(
            checked_write_bounds(100, 1000, true, Slot::A, 50, 2000).unwrap(),
            1000
        );
        assert_eq!(
            checked_write_bounds(100, 1000, true, Slot::B, 50, 2000).unwrap(),
            1100
        );
    }

    #[test]
    fn write_ending_exactly_at_the_end_of_the_device_is_allowed() {
        assert_eq!(
            checked_write_bounds(0, 900, false, Slot::A, 100, 1000).unwrap(),
            900
        );
    }

    #[test]
    fn write_exceeding_the_device_is_rejected() {
        assert!(checked_write_bounds(0, 900, false, Slot::A, 101, 1000).is_err());
    }

    #[test]
    fn overflowing_redundant_slot_offset_is_rejected() {
        assert!(checked_write_bounds(u64::MAX, 1, true, Slot::B, 0, u64::MAX).is_err());
    }

    #[test]
    fn overflowing_write_range_is_rejected() {
        // Wrapping `src_len + offset` to 0 would make the bounds check trivially pass.
        assert!(
            checked_write_bounds(0, 1, false, Slot::A, u64::MAX, u64::MAX).is_err()
        );
    }
}
