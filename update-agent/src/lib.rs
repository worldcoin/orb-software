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

/// Writes a serializable value as JSON to the given path and syncs to disk.
pub fn write_json_and_sync(
    path: &Path,
    value: &impl serde::Serialize,
) -> eyre::Result<()> {
    let file = File::options()
        .write(true)
        .read(true)
        .create(true)
        .truncate(true)
        .open(path)
        .wrap_err_with(|| format!("failed to open `{}`", path.display()))?;
    serde_json::to_writer(&file, value)
        .wrap_err_with(|| format!("failed to write JSON to `{}`", path.display()))?;
    file.sync_all()
        .wrap_err_with(|| format!("failed to sync `{}` to disk", path.display()))?;
    Ok(())
}

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
    write_json_and_sync(path, &version_map)
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
