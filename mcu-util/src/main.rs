extern crate core;
use crate::orb::Orb;
use clap::Parser;
use eyre::{Context, Result};
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;
use tracing::{debug, error};

mod logging;
mod messaging;
mod orb;

/// Utility args
#[derive(Parser, Debug)]
#[clap(
    author,
    version,
    about = "Orb MCU utility",
    long_about = "Debug microcontrollers and manage firmware images"
)]
struct Args {
    #[clap(subcommand)]
    subcmd: SubCommand,
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
    /// Use CAN-FD to send the image
    #[clap(short, long, default_value = "false")]
    can_fd: bool,
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

/// Commands to the secure element
#[derive(Parser, Debug)]
enum SecureElement {
    /// Request power-cycling of the secure element
    #[clap(action)]
    PowerCycle,
}

#[cfg(debug_assertions)]
const LOG_LEVEL: tracing::Level = tracing::Level::DEBUG;
#[cfg(not(debug_assertions))]
const LOG_LEVEL: tracing::Level = tracing::Level::INFO;

async fn execute(args: Args) -> Result<()> {
    let mut orb = Orb::new().await?;

    match args.subcmd {
        SubCommand::Info => {
            let orb_info = orb.get_info().await?;
            debug!("{:?}", orb_info);
            println!("{:#}", orb_info);
            Ok(())
        }
        SubCommand::Reboot(mcu) => orb.borrow_mut_mcu(mcu).reboot(None).await,
        SubCommand::Dump(DumpOpts {
            mcu,
            duration,
            logs_only,
        }) => {
            orb.borrow_mut_mcu(mcu)
                .dump(duration.map(Duration::from_secs), logs_only)
                .await
        }
        SubCommand::Stress(StressOpts { duration, mcu }) => {
            orb.borrow_mut_mcu(mcu)
                .stress_test(duration.map(Duration::from_secs))
                .await
        }
        SubCommand::Image(Image::Switch(mcu)) => {
            orb.borrow_mut_mcu(mcu).switch_images().await
        }
        SubCommand::Image(Image::Update(opts)) => {
            orb.borrow_mut_mcu(opts.mcu)
                .update_firmware(&opts.path, opts.can_fd)
                .await
        }
        SubCommand::HardwareRevision { filename } => {
            let hw_str = orb.get_revision().await?;
            match filename {
                None => {
                    println!("{}", hw_str);
                }
                Some(ref filename) => {
                    let mut file =
                        std::fs::File::create(filename).with_context(|| {
                            format!("Failed to create file {filename:?}")
                        })?;
                    write!(file, "{}", hw_str).with_context(|| {
                        format!("Failed to write to file {filename:?}")
                    })?;
                }
            }
            Ok(())
        }
        SubCommand::SecureElement(opts) => match opts {
            SecureElement::PowerCycle => {
                orb.borrow_mut_sec_board()
                    .power_cycle_secure_element()
                    .await
            }
        },
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    logging::init(LOG_LEVEL)?;

    let args = Args::parse();

    if cfg!(debug_assertions) {
        debug!("{:?}", args);
    }

    if let Err(e) = execute(args).await {
        error!("{}", e);
        std::process::exit(-1);
    } else {
        std::process::exit(0);
    }
}
