use clap::{Parser, Subcommand};
use orb_slot_ctrl::{RootFsStatus, Slot};
use std::{env, process::exit};

use orb_build_info::{make_build_info, BuildInfo};

const BUILD_INFO: BuildInfo = make_build_info!();

#[derive(Parser)]
#[command(
    author,
    version = BUILD_INFO.version,
    long_about = "This tool is designed to read and write the slot and rootfs state of the Orb."
)]
struct Cli {
    #[command(subcommand)]
    subcmd: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Get the current active slot.
    #[command(name = "current", short_flag = 'c')]
    GetSlot,
    /// Get the slot set for the next boot.
    #[command(name = "next", short_flag = 'n')]
    GetNextSlot,
    /// Set slot for the next boot.
    #[command(name = "set", short_flag = 's')]
    SetNextSlot {
        #[arg(
            value_parser = clap::value_parser!(Slot),
            help = Slot::help_message()
        )]
        slot: Slot,
    },
    /// Rootfs status controls.
    Status {
        /// Control the inactive slot instead of the active.
        #[arg(long = "inactive", short = 'i')]
        inactive: bool,
        #[command(subcommand)]
        subcmd: StatusCommands,
    },
    /// Get the git commit used for this build.
    #[command(name = "git", short_flag = 'g')]
    GitDescribe,
}

#[derive(Subcommand)]
enum StatusCommands {
    /// Get the rootfs status.
    #[command(name = "get", short_flag = 'g')]
    GetRootfsStatus,
    /// Set the rootfs status.
    #[command(name = "set", short_flag = 's')]
    SetRootfsStatus {
        #[arg(
            value_parser = clap::value_parser!(RootFsStatus),
            help = RootFsStatus::help_message()
        )]
        status: RootFsStatus,
    },
    /// Get the retry counter.
    #[command(name = "retries", short_flag = 'c')]
    GetRetryCounter,
    /// Set the retry counter to maximum.
    #[command(name = "reset", short_flag = 'r')]
    ResetRetryCounter,
    /// Get the maximum retry counter.
    #[command(name = "max", short_flag = 'm')]
    GetMaxRetryCounter,
    /// Get a full list of rootfs status variants.
    #[command(name = "list", short_flag = 'l')]
    ListStatusVariants,
}

fn check_running_as_root(error: orb_slot_ctrl::Error) {
    let uid = unsafe { libc::getuid() };
    let euid = unsafe { libc::geteuid() };
    if !matches!((uid, euid), (0, 0)) {
        println!("Please try again as root user.");
        exit(1)
    }
    panic!("{}", error)
}

fn main() -> eyre::Result<()> {
    let cli = Cli::parse();
    match cli.subcmd {
        Commands::GetSlot => {
            println!("{}", orb_slot_ctrl::get_current_slot()?);
        }
        Commands::GetNextSlot => {
            println!("{}", orb_slot_ctrl::get_next_boot_slot()?);
        }
        Commands::SetNextSlot { slot } => {
            if let Err(e) = orb_slot_ctrl::set_next_boot_slot(slot) {
                check_running_as_root(e);
            };
        }
        Commands::Status { inactive, subcmd } => match subcmd {
            StatusCommands::GetRootfsStatus => {
                if inactive {
                    println!(
                        "{:?}",
                        orb_slot_ctrl::get_rootfs_status(
                            orb_slot_ctrl::get_inactive_slot()?
                        )?
                    );
                } else {
                    println!("{:?}", orb_slot_ctrl::get_current_rootfs_status()?);
                }
            }
            StatusCommands::SetRootfsStatus { status } => {
                if inactive {
                    if let Err(e) = orb_slot_ctrl::set_rootfs_status(
                        status,
                        orb_slot_ctrl::get_inactive_slot()?,
                    ) {
                        check_running_as_root(e);
                    }
                } else if let Err(e) = orb_slot_ctrl::set_current_rootfs_status(status)
                {
                    check_running_as_root(e);
                }
            }
            StatusCommands::GetRetryCounter => {
                if inactive {
                    println!(
                        "{}",
                        orb_slot_ctrl::get_retry_count(
                            orb_slot_ctrl::get_inactive_slot()?
                        )?
                    );
                } else {
                    println!("{}", orb_slot_ctrl::get_current_retry_count()?);
                }
            }
            StatusCommands::GetMaxRetryCounter => {
                println!("{}", orb_slot_ctrl::get_max_retry_count()?);
            }
            StatusCommands::ResetRetryCounter => {
                if inactive {
                    if let Err(e) = orb_slot_ctrl::reset_retry_count_to_max(
                        orb_slot_ctrl::get_inactive_slot()?,
                    ) {
                        check_running_as_root(e)
                    }
                } else if let Err(e) = orb_slot_ctrl::reset_current_retry_count_to_max()
                {
                    check_running_as_root(e)
                }
            }
            StatusCommands::ListStatusVariants => {
                println!("Available Rootfs status variants with their aliases:");
                println!("{}", RootFsStatus::help_message());
            }
        },
        Commands::GitDescribe => {
            println!("{}", BUILD_INFO.git.describe);
        }
    }
    Ok(())
}
