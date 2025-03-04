#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::time::Duration;

use clap::{
    builder::{styling::AnsiColor, Styles},
    Parser,
};
use color_eyre::eyre::{Context, Result};
use orb_build_info::{make_build_info, BuildInfo};
use orb_mcu_interface::orb_messages::hardware::OrbVersion;
use tokio::fs;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use tracing::{debug, error};

use crate::orb::Orb;

mod orb;

static BUILD_INFO: BuildInfo = make_build_info!();

/// Utility args
#[derive(Parser, Debug)]
#[clap(
    author,
    version = BUILD_INFO.version,
    about = "Orb MCU utility",
    long_about = "Debug microcontrollers and manage firmware images",
    styles = clap_v3_styles(),
)]
struct Args {
    #[clap(subcommand)]
    subcmd: SubCommand,
    #[clap(short, long, default_value = "false")]
    can_fd: bool,
}

#[derive(Parser, Debug)]
enum SubCommand {
    /// Print Orb's state data
    #[clap(action)]
    Info,
    /// Reboot a microcontroller. Rebooting the main MCU can be used to reboot the Orb.
    #[clap(subcommand)]
    Reboot(Mcu),
    /// Firmware image handling
    #[clap(subcommand)]
    Image(Image),
    /// Dump one microcontroller messages
    #[clap(action)]
    Dump(DumpOpts),
    /// Stress microcontroller by flooding communication channels
    #[clap(action)]
    Stress(StressOpts),
    /// Control optics: gimbal
    #[clap(subcommand)]
    Optics(OpticsOpts),
    /// Control secure element
    #[clap(subcommand)]
    SecureElement(SecureElement),
    /// Prints hardware revision from main MCU in machine-readable form
    #[clap(action)]
    HardwareRevision {
        ///Path to file to write hardware revision to. If not specified, revision is printed to stdout.
        #[clap(long)]
        filename: Option<PathBuf>,
    },
}

#[derive(Parser, Debug)]
pub struct DumpOpts {
    /// Microcontroller
    #[clap(subcommand)]
    mcu: Mcu,
    /// Dump duration in seconds (minimum 10 seconds)
    #[clap(short, long)]
    duration: Option<u64>,
    /// Print only logs from the microcontroller to stdout
    #[clap(short, long, default_value = "false")]
    logs_only: bool,
}

#[derive(Parser, Debug)]
enum Image {
    /// Switch images in slots to revert or update a newly transferred image
    #[clap(subcommand)]
    Switch(Mcu),
    /// Update microcontroller's firmware
    #[clap(action)]
    Update(McuUpdate),
}

/// Mcu Update options
#[derive(Parser, Debug)]
pub struct McuUpdate {
    /// Mcu
    #[clap(subcommand)]
    mcu: Mcu,
    /// Path to binary file
    #[clap(short, long)]
    path: String,
}

/// Stress tests options
#[derive(Parser, Debug)]
pub struct StressOpts {
    /// Stress test duration in seconds (minimum 10 seconds)
    #[clap(short, long)]
    duration: Option<u64>,
    /// Microcontroller to perform the test on
    #[clap(subcommand)]
    mcu: Mcu,
}

/// Select microcontroller
#[derive(Parser, Debug, Clone, Copy, PartialEq)]
pub enum Mcu {
    /// Main microcontroller
    #[clap(action)]
    Main = 0x01,
    /// Security microcontroller
    #[clap(action)]
    Security = 0x02,
}

/// Optics tests options
#[derive(Parser, Debug, Clone, Copy)]
enum OpticsOpts {
    /// Auto-home the gimbal
    #[clap(action)]
    GimbalHome,
    /// Set gimbal position: --phi and --theta
    #[clap(action)]
    GimbalPosition(OpticsPosition),
    /// Test camera trigger for 10 seconds with default options: 30fps, IR-LEDs 100us.
    #[clap(subcommand)]
    TriggerCamera(Camera),
}

#[derive(Parser, Debug, Clone, Copy)]
enum Camera {
    #[clap(action)]
    Eye,
    #[clap(action)]
    Face,
}

/// Optics position
#[derive(Parser, Debug, Clone, Copy)]
struct OpticsPosition {
    /// Move mirror right/left. Angle in millidegrees. Center is 45000.
    #[clap(short, long)]
    phi: u32,
    /// Move mirror up/down. Angle in millidegrees. Center is 90000.
    #[clap(short, long)]
    theta: u32,
}

/// Commands to the secure element
#[derive(Parser, Debug)]
enum SecureElement {
    /// Request power-cycling of the secure element
    #[clap(action)]
    PowerCycle,
}

async fn execute(args: Args) -> Result<()> {
    let (mut orb, orb_tasks) = Orb::new(args.can_fd).await?;

    match args.subcmd {
        SubCommand::Info => {
            let orb_info = orb.get_info().await?;
            debug!("{:?}", orb_info);
            println!("{:#}", orb_info);
        }
        SubCommand::Reboot(mcu) => orb.board_mut(mcu).reboot(None).await?,
        SubCommand::Dump(DumpOpts {
            mcu,
            duration,
            logs_only,
        }) => {
            orb.board_mut(mcu)
                .dump(duration.map(Duration::from_secs), logs_only)
                .await?
        }
        SubCommand::Stress(StressOpts { duration, mcu }) => {
            orb.board_mut(mcu)
                .stress_test(duration.map(Duration::from_secs))
                .await?
        }
        SubCommand::Image(Image::Switch(mcu)) => {
            orb.board_mut(mcu).switch_images().await?
        }
        SubCommand::Image(Image::Update(opts)) => {
            orb.board_mut(opts.mcu).update_firmware(&opts.path).await?
        }
        SubCommand::HardwareRevision { filename } => {
            let hw_rev = orb.get_revision().await?;
            // discard operation if unknown hardware version
            if hw_rev.0.version == i32::from(OrbVersion::HwVersionUnknown) {
                return Err(color_eyre::eyre::eyre!(
                    "Failed to fetch hardware revision: unknown"
                ));
            }
            match filename {
                None => {
                    println!("{}", hw_rev);
                }
                Some(ref filename) => {
                    let hw_str = format!("{}", hw_rev);
                    // check that the file exists and compare content with what's going to be
                    // written to avoid writing the same content.
                    if let Ok(existing_content) = fs::read_to_string(filename)
                        .await
                        .with_context(|| format!("Failed to read file {filename:?}"))
                    {
                        if existing_content == hw_str {
                            debug!("Content is the same, skipping write");
                            return Ok(());
                        }
                    }

                    // overwrite the file with the new content
                    let mut file = OpenOptions::new()
                        .read(true)
                        .write(true)
                        .truncate(true)
                        .create(true)
                        .open(filename)
                        .await
                        .with_context(|| format!("Failed to open file {filename:?}"))?;
                    file.set_len(0).await?;
                    if file.write(hw_str.as_bytes()).await? != hw_str.len() {
                        return Err(color_eyre::eyre::eyre!("Failed to write to file"));
                    }
                    file.flush().await?;
                }
            }
        }
        SubCommand::Optics(opts) => match opts {
            OpticsOpts::GimbalHome => orb.main_board_mut().gimbal_auto_home().await?,
            OpticsOpts::GimbalPosition(opts) => {
                orb.main_board_mut()
                    .gimbal_set_position(opts.phi, opts.theta)
                    .await?
            }
            OpticsOpts::TriggerCamera(camera) => {
                orb.main_board_mut().trigger_camera(camera).await?
            }
        },
        SubCommand::SecureElement(opts) => match opts {
            SecureElement::PowerCycle => {
                orb.sec_board_mut().power_cycle_secure_element().await?
            }
        },
    }

    // Kills the tasks
    drop(orb);
    // Timeout because tasks might never check the kill signal because they are busy
    // waiting to receive another message. In the event we timeout, most likely there
    // have been no errors.
    // TODO: We need to make the synchronous actually use nonblocking code to make the
    // timeout unecessary
    match tokio::time::timeout(Duration::from_millis(100), orb_tasks.join()).await {
        Ok(result) => result,
        Err(tokio::time::error::Elapsed { .. }) => Ok(()),
    }
}

fn clap_v3_styles() -> Styles {
    Styles::styled()
        .header(AnsiColor::Yellow.on_default())
        .usage(AnsiColor::Green.on_default())
        .literal(AnsiColor::Green.on_default())
        .placeholder(AnsiColor::Green.on_default())
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let telemetry = orb_telemetry::TelemetryConfig::new().init();

    let args = Args::parse();

    if cfg!(debug_assertions) {
        debug!("{:?}", args);
    }

    if let Err(e) = execute(args).await {
        error!("{:#?}", e);
        telemetry.flush().await;
        std::process::exit(-1);
    } else {
        telemetry.flush().await;
        std::process::exit(0);
    }
}
