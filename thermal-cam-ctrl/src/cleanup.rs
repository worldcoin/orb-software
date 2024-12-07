use clap::Parser;
use color_eyre::Result;
use eyre::{OptionExt as _, WrapErr as _};
use seek_camera::{
    manager::{CameraHandle, Event, Manager},
    ChipId, ErrorCode,
};

use crate::{start_manager, Flow};

/// Manages pairing of the camera
#[derive(Debug, Parser)]
pub struct Cleanup {}

impl Cleanup {
    pub fn run(self) -> Result<()> {
        start_manager(Box::new(on_cam))
    }
}

fn on_cam(
    mngr: &mut Manager,
    cam_h: CameraHandle,
    evt: Event,
    _err_code: Option<ErrorCode>,
) -> Result<Flow> {
    match evt {
        Event::Connect | Event::ReadyToPair => (),
        _ => return Ok(Flow::Finish),
    }
    let cid = mngr
        .cameras()
        .wrap_err("failed to get cameras")?
        .get_mut(&cam_h)
        .ok_or_eyre("failed to access camera from handle")?
        .chip_id()
        .wrap_err("failed to get camera chip_id")?;
    delete_other_cams(&cid)?;
    Ok(Flow::Finish)
}

fn delete_other_cams(cid_to_keep: &ChipId) -> Result<()> {
    let calib_dir = crate::get_seek_dir().join("cal");
    for entry in calib_dir
        .read_dir()
        .wrap_err("failed to access SEEKTHERMAL_ROOT/.seekthermal/cal")?
    {
        let entry = entry?;
        if cid_to_keep.as_str() == entry.file_name() {
            continue; // skips matching dir
        }
        tracing::info!("removing {}", entry.path().display());
        std::fs::remove_dir_all(entry.path()).wrap_err_with(|| {
            format!(
                "failed to delete directory contents at {}",
                entry.path().display()
            )
        })?
    }
    Ok(())
}
