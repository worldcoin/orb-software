use std::{
    ffi::{CStr, CString},
    path::PathBuf,
    sync::OnceLock,
};

use clap::{Args, Subcommand};
use color_eyre::{
    eyre::{eyre, WrapErr},
    owo_colors::{AnsiColors, OwoColorize},
    Result,
};
use indicatif::ProgressBar;
use seek_camera::manager::{CameraHandle, Event, Manager};
use tracing::info;

use crate::{start_manager, Flow};

/// Manages pairing of the camera
#[derive(Debug, Args)]
pub struct Pairing {
    #[clap(subcommand)]
    commands: Commands,
}

impl Pairing {
    pub fn run(self) -> Result<()> {
        match self.commands {
            Commands::Status(c) => c.run(),
            Commands::Pair(c) => c.run(),
        }
    }
}

#[derive(Debug, Subcommand)]
enum Commands {
    Status(Status),
    Pair(Pair),
}

/// Checks the pairing status
#[derive(Debug, Args)]
struct Status {
    /// Continue to check for new camera events even after the first one.
    #[clap(short)]
    continue_running: bool,
}

impl Status {
    fn run(self) -> Result<()> {
        let cam_fn = move |mngr: &mut _, cam_h, evt, _err| {
            helper(
                mngr,
                cam_h,
                evt,
                PairingBehavior::DoNothing,
                self.continue_running,
                None,
            )
        };
        start_manager(Box::new(cam_fn))
    }
}

/// Pairs camera(s)
#[derive(Debug, Args)]
struct Pair {
    /// Forces cameras already paired to re-pair.
    #[clap(short)]
    force_pair: bool,
    /// Continue to check for new camera events even after the first one.
    #[clap(short)]
    continue_running: bool,
    #[clap(long)]
    from_dir: Option<PathBuf>,
}

impl Pair {
    fn run(self) -> Result<()> {
        let from_dir = self
            .from_dir
            .map(|p| CString::new(p.to_string_lossy().into_owned()).unwrap());
        let pairing_behavior = if self.force_pair {
            PairingBehavior::ForcePair
        } else {
            PairingBehavior::Pair
        };
        let cam_fn = move |mngr: &mut _, cam_h, evt, _err| {
            helper(
                mngr,
                cam_h,
                evt,
                pairing_behavior,
                self.continue_running,
                from_dir.as_deref(),
            )
        };
        start_manager(Box::new(cam_fn))
    }
}

///////////////////////////
// ---- Helper Code ---- //
///////////////////////////

/// Used to control the pairing behavior of [`helper`].
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum PairingBehavior {
    DoNothing,
    /// Pairs cameras that are not yet paired.
    Pair,
    /// Pairs all cameras, even if they were already paired.
    ForcePair,
}

/// Because [`Status`] and [`Pair`] have such similar logic, we use this function
/// to easily reuse code.
fn helper(
    mngr: &mut Manager,
    cam_h: CameraHandle,
    evt: Event,
    pairing_behavior: PairingBehavior,
    continue_running: bool,
    from_dir: Option<&CStr>,
) -> Result<Flow> {
    let is_paired = match evt {
        Event::Connect => true,
        Event::Disconnect => return Ok(Flow::Continue),
        Event::ReadyToPair => false,
        Event::Error => return Ok(Flow::Continue),
    };
    let mut cams = mngr.cameras().unwrap();
    let cam = cams
        .get_mut(&cam_h)
        .ok_or_else(|| eyre!("failed to get camera from handle"))?;

    let serial = cam
        .serial_number()
        .wrap_err("Failed to get serial number")?;
    let cid = cam.chip_id().wrap_err("Failed to get chip id")?;

    let paired = if is_paired {
        "paired".color(AnsiColors::Green)
    } else {
        "unpaired".color(AnsiColors::Red)
    };
    info!("Found {paired} camera with cid: {cid}, serial: {serial}");

    if pairing_behavior == PairingBehavior::ForcePair
        || pairing_behavior == PairingBehavior::Pair && !is_paired
    {
        info!("Pairing camera (cid {cid})...");
        cam.store_calibration_data(from_dir, Some(pair_progress_cb))
            .wrap_err("Error while pairing camera")?;
        info!("{} camera (cid {cid})", "Paired".green());
    }

    if continue_running {
        Ok(Flow::Continue)
    } else {
        Ok(Flow::Finish)
    }
}

fn pair_progress_cb(pct: u8) {
    static BAR: OnceLock<ProgressBar> = OnceLock::new();
    BAR.get_or_init(|| ProgressBar::new(100))
        .set_position(pct as u64);
}
