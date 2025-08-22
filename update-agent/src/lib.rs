pub mod claim;
pub mod client;
pub mod component;
pub mod dbus;
pub mod json;
pub mod manifest;
pub mod mount;
pub mod settings;
pub mod update;
pub mod util;

use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

use component::Component;
use eyre::{ensure, WrapErr as _};
use orb_build_info::{make_build_info, BuildInfo};
use orb_update_agent_core::{Slot, VersionMap};
pub use settings::{Args, Settings};

pub const BUILD_INFO: BuildInfo = make_build_info!();

pub fn update_component_version_on_disk(
    target_slot: Slot,
    component: &Component,
    version_map: &mut VersionMap,
    path: &Path,
) -> eyre::Result<()> {
    version_map.set_component(
        target_slot,
        component.manifest_component(),
        component.system_component(),
    );
    serde_json::to_writer(
        &File::options()
            .create(true)
            .write(true)
            .read(true)
            .truncate(true)
            .open(path)
            .wrap_err("failed opening versions file")?,
        &version_map,
    )
    .wrap_err("failed writing versions to file")?;
    Ok(())
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

/// Re-export commonly used types for convenience
pub use dbus::interfaces::UpdateProgress as UpdateAgentProgress;
pub use orb_update_agent_dbus::{ComponentState, ComponentStatus, UpdateAgentState};
