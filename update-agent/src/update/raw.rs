use eyre::{ensure, WrapErr as _};
use orb_update_agent_core::{
    components,
    telemetry::{LogOnError, DATADOG},
    Slot,
};
use std::io::{self, Seek as _, Write};
use tracing::debug;

use super::Update;

impl Update for components::Raw {
    fn update<R>(&self, slot: Slot, mut src: R) -> eyre::Result<()>
    where
        R: io::Read + io::Seek,
    {
        DATADOG
            .incr("orb.update.count.component.raw", ["status:started"])
            .or_log();
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

        let offset = if slot == Slot::B && self.is_redundant() {
            self.size + self.offset
        } else {
            self.offset
        };
        debug!("-- setting up offset to be {:?}", offset);

        ensure!(
            block_dev_len >= src_len + offset,
            "block device is too small to write {} bytes starting at offset {}",
            src_len,
            offset,
        );
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
            .map_err({
                DATADOG
                    .incr("orb.update.count.component.raw", ["status:write_error"])
                    .or_log();
                |e| e
            })?;
        debug!("-- copied!");

        block_dev
            .flush()
            .wrap_err_with(|| format!("block device `{}` flush failed", self.device))?;
        debug!("-- flushed!");

        DATADOG
            .incr("orb.update.count.component.raw", ["status:write_complete"])
            .or_log();

        Ok(())
    }
}
