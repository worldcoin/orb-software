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

/// Common update-related utilities that can be shared across orb components
pub mod common_utils {
    use orb_update_agent_dbus::{ComponentState, UpdateAgentState};

    /// Maps UpdateAgentState values to their numeric representation
    pub struct UpdateAgentStateMapper;

    impl UpdateAgentStateMapper {
        pub fn from_u32(value: u32) -> Option<UpdateAgentState> {
            match value {
                1 => Some(UpdateAgentState::None),
                2 => Some(UpdateAgentState::Downloading),
                3 => Some(UpdateAgentState::Fetched),
                4 => Some(UpdateAgentState::Processed),
                5 => Some(UpdateAgentState::Installing),
                6 => Some(UpdateAgentState::Installed),
                7 => Some(UpdateAgentState::Rebooting),
                8 => Some(UpdateAgentState::NoNewVersion),
                _ => None,
            }
        }

        pub fn to_u32(state: UpdateAgentState) -> u32 {
            match state {
                UpdateAgentState::None => 1,
                UpdateAgentState::Downloading => 2,
                UpdateAgentState::Fetched => 3,
                UpdateAgentState::Processed => 4,
                UpdateAgentState::Installing => 5,
                UpdateAgentState::Installed => 6,
                UpdateAgentState::Rebooting => 7,
                UpdateAgentState::NoNewVersion => 8,
            }
        }
    }

    /// Maps ComponentState values  
    pub struct ComponentStateMapper;

    impl ComponentStateMapper {
        pub fn from_update_agent_state(state: UpdateAgentState) -> ComponentState {
            match state {
                UpdateAgentState::None => ComponentState::None,
                UpdateAgentState::Downloading => ComponentState::Downloading,
                UpdateAgentState::Fetched => ComponentState::Fetched,
                UpdateAgentState::Processed => ComponentState::Processed,
                UpdateAgentState::Installing => ComponentState::Installing,
                UpdateAgentState::Installed => ComponentState::Installed,
                UpdateAgentState::Rebooting => ComponentState::Installed, // Map rebooting to installed
                UpdateAgentState::NoNewVersion => ComponentState::None,
            }
        }
    }
}

/// Re-export commonly used types for convenience
pub use dbus::interfaces::UpdateProgress as UpdateAgentProgress;
pub use orb_update_agent_dbus::{ComponentState, ComponentStatus, UpdateAgentState};
