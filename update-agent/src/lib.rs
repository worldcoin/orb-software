pub mod claim;
pub mod client;
pub mod component;
pub mod dbus;
pub mod json;
pub mod logging;
pub mod manifest;
pub mod mount;
pub mod settings;
pub mod update;
pub mod util;

use std::{fs::File, path::Path};

use component::Component;
use eyre::WrapErr as _;
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
