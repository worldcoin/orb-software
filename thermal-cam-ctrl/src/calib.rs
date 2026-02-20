use clap::{Args, Subcommand};
use color_eyre::{
    eyre::{bail, eyre},
    Result,
};
use indicatif::ProgressBar;
use orb_info::OrbId;
use seek_camera::{
    filters::FlatSceneCorrectionId,
    frame_format::FrameFormat,
    manager::{CameraHandle, Manager},
};
use std::{sync::OnceLock, time::Duration};
use tracing::info;

use crate::{health, start_manager, Flow};

#[derive(Debug, Args)]
pub struct Calibration {
    #[clap(subcommand)]
    commands: Commands,
}

impl Calibration {
    pub fn run(self, orb_id: Option<&OrbId>) -> Result<()> {
        match self.commands {
            Commands::Fsc(c) => c.run(orb_id),
        }
    }
}

#[derive(Debug, Subcommand)]
enum Commands {
    Fsc(Fsc),
}

#[derive(Debug, Args)]
pub struct Fsc {
    /// Warmup time in seconds before starting the flat scene calibration.
    #[arg(default_value_t = 4)]
    warmup_time: u32,
    /// Deletes the existing FSC
    #[arg(short, long)]
    delete: bool,
}

impl Fsc {
    pub fn run(self, orb_id: Option<&OrbId>) -> Result<()> {
        if self.delete {
            start_manager(
                Box::new(move |mngr, cam_h, _evt, _err| delete_fsc(mngr, cam_h)),
                None,
            )
        } else {
            let warmup_time = Duration::from_secs(self.warmup_time.into());
            let orb_id = orb_id.cloned();
            start_manager(
                Box::new(move |mngr, cam_h, _evt, _err| {
                    new_fsc(mngr, cam_h, warmup_time, orb_id.as_ref())
                }),
                None,
            )
        }
    }
}

fn delete_fsc(mngr: &mut Manager, cam_h: CameraHandle) -> Result<Flow> {
    let mut cams = mngr.cameras().unwrap();
    let cam = cams
        .get_mut(&cam_h)
        .ok_or_else(|| eyre!("failed to get camera from handle"))?;

    // This static is necessary because we don't support closures for the progress
    // callback.
    static BAR: OnceLock<ProgressBar> = OnceLock::new();
    cam.delete_flat_scene_correction(
        FlatSceneCorrectionId::_0,
        Some(|pct| {
            BAR.get_or_init(|| ProgressBar::new(100))
                .set_position(pct as u64)
        }),
    )?;
    info!("Completed deletion!");
    Ok(Flow::Finish)
}

fn new_fsc(
    mngr: &mut Manager,
    cam_h: CameraHandle,
    warmup_time: Duration,
    orb_id: Option<&OrbId>,
) -> Result<Flow> {
    let mut cams = mngr.cameras().unwrap();
    let cam = cams
        .get_mut(&cam_h)
        .ok_or_else(|| eyre!("failed to get camera from handle"))?;
    if !cam.is_paired() {
        bail!(
            "Camera should be paired first, before performing a flat scene calibration"
        );
    }

    cam.capture_session_start(FrameFormat::Grayscale)?;
    info!("Warming camera up for {} seconds.", warmup_time.as_secs());
    std::thread::sleep(warmup_time);

    // This static is necessary because we don't support closures for the progress
    // callback.
    static BAR: OnceLock<ProgressBar> = OnceLock::new();
    info!("Beginning flat scene calibration.");
    cam.store_flat_scene_correction(
        FlatSceneCorrectionId::_0,
        Some(|pct| {
            BAR.get_or_init(|| ProgressBar::new(100))
                .set_position(pct as u64)
        }),
    )?;
    cam.capture_session_stop()?;
    info!("Completed calibration!");

    if let Some(orb_id) = orb_id {
        health::publish_calibration_status(
            orb_id,
            "success",
            "calibrated and verified",
        );
    }

    Ok(Flow::Finish)
}
