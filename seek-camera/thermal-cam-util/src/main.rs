mod calib;
mod capture;
mod log;
mod pairing;

use std::{
    path::{Path, PathBuf},
    sync::{mpsc, OnceLock},
};

use clap::{
    builder::{styling::AnsiColor, Styles},
    Parser, Subcommand,
};
use color_eyre::{
    eyre::{eyre, WrapErr},
    Help, Result,
};
use owo_colors::{AnsiColors, OwoColorize};
use seek_camera::{
    manager::{CameraHandle, Event, Manager},
    ErrorCode,
};

static SEEK_DIR: OnceLock<PathBuf> = OnceLock::new();

fn make_clap_v3_styles() -> Styles {
    Styles::styled()
        .header(AnsiColor::Yellow.on_default())
        .usage(AnsiColor::Green.on_default())
        .literal(AnsiColor::Green.on_default())
        .placeholder(AnsiColor::Green.on_default())
}

#[derive(Debug, Parser)]
#[command(about, author, version=env!("GIT_VERSION"), styles=make_clap_v3_styles())]
struct Cli {
    #[clap(subcommand)]
    commands: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Calibration(crate::calib::Calibration),
    Capture(crate::capture::Capture),
    Log(crate::log::Log),
    Pairing(crate::pairing::Pairing),
}

fn get_seek_dir() -> &'static Path {
    SEEK_DIR.get_or_init(|| {
        let default_seek_dir =
            PathBuf::from(std::env::var("HOME").unwrap_or("~".to_string()));
        #[cfg(windows)]
        let default_seek_dir = PathBuf::from(
            std::env::var("APPDATA").expect("expected %APPDATA% to be set"),
        );
        let root = std::env::var("SEEKTHERMAL_ROOT")
            .map(PathBuf::from)
            .unwrap_or(default_seek_dir);

        #[cfg(unix)]
        return root.join(".seekthermal");
        #[cfg(windows)]
        return root.join("SeekThermal");
    })
}

/// Used in [`start_manager`].
type OnCamFn = Box<
    dyn FnMut(&mut Manager, CameraHandle, Event, Option<ErrorCode>) -> Result<Flow>,
>;

/// Forwards events from the [`Manager`] to `on_cam`.
fn start_manager(mut on_cam: OnCamFn) -> Result<()> {
    let mut mngr = Manager::new().wrap_err("Failed to create camera manager")?;

    let (send, recv) = mpsc::channel();
    mngr.set_callback(move |cam_h, evt, err| {
        let _ = send.send((cam_h, evt, err));
    })
    .expect("Should be able to set manager callback");

    loop {
        let (cam_h, evt, err) = recv
            .recv()
            .wrap_err("Unexpected disconnection from manager callback")?;
        let flow = on_cam(&mut mngr, cam_h, evt, err)?;
        match flow {
            Flow::Continue => continue,
            Flow::Finish => return Ok(()),
        }
    }
}

/// Used to control the control flow of [`start_manager`].
enum Flow {
    Continue,
    Finish,
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Cli::parse();
    if std::env::var("SEEKTHERMAL_ROOT").unwrap_or_default() == "" {
        return Err(eyre!("`SEEKTHERMAL_ROOT` env var must be explicitly set!"))
            .suggestion(
                "Set `SEEKTHERMAL_ROOT` to the same value that `orb-core` uses!",
            );
    }

    #[cfg(windows)]
    const USER_ENV_VAR: &str = "UserName";
    #[cfg(unix)]
    const USER_ENV_VAR: &str = "USER";
    if std::env::var(USER_ENV_VAR).unwrap_or_default() == "root" {
        eprintln!(
            "{}",
            "warning: running as root. This may mess up file permissions."
                .color(AnsiColors::Red)
        );
    }

    #[cfg(unix)]
    match args.commands {
        Commands::Calibration(c) => c.run(),
        Commands::Capture(c) => c.run(),
        Commands::Log(c) => c.run(),
        Commands::Pairing(c) => c.run(),
    }
}
