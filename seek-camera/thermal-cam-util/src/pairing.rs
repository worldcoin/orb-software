use std::sync::OnceLock;

use clap::{Args, Subcommand};
use color_eyre::{
    eyre::WrapErr,
    owo_colors::{AnsiColors, OwoColorize},
    Result,
};
use indicatif::ProgressBar;
use seek_camera::manager::{CameraHandle, Event, Manager};

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
}

impl Pair {
    fn run(self) -> Result<()> {
        let pairing_behavior = if self.force_pair {
            PairingBehavior::ForcePair
        } else {
            PairingBehavior::Pair
        };
        let cam_fn = move |mngr: &mut _, cam_h, evt, _err| {
            helper(mngr, cam_h, evt, pairing_behavior, self.continue_running)
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
) -> Result<Flow> {
    let is_paired = match evt {
        Event::Connect => true,
        Event::Disconnect => return Ok(Flow::Continue),
        Event::ReadyToPair => false,
        Event::Error => return Ok(Flow::Continue),
    };

    let serial = mngr
        .camera_mut(cam_h, |cam| cam.unwrap().serial_number())
        .wrap_err("Failed to get serial number")?;
    let cid = mngr
        .camera_mut(cam_h, |cam| cam.unwrap().chip_id())
        .wrap_err("Failed to get chip id")?;

    let paired = if is_paired {
        "paired".color(AnsiColors::Green)
    } else {
        "unpaired".color(AnsiColors::Red)
    };
    println!("Found {paired} camera with cid: {cid}, serial: {serial}");

    if pairing_behavior == PairingBehavior::ForcePair
        || pairing_behavior == PairingBehavior::Pair && !is_paired
    {
        println!("Pairing camera (cid {cid})...");
        mngr.camera_mut(cam_h, |cam| {
            cam.unwrap()
                .store_calibration_data(None, Some(pair_progress_cb))
        })
        .wrap_err("Error while pairing camera")?;
        println!("{} camera (cid {cid})", "Paired".green());
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
