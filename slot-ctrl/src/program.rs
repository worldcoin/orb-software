use crate::OrbSlotCtrl;
use clap::{Parser, Subcommand};
use orb_build_info::{make_build_info, BuildInfo};
use std::{env, process::exit};

const BUILD_INFO: BuildInfo = make_build_info!();

#[derive(Parser)]
#[command(
    author,
    version = BUILD_INFO.version,
    long_about = "This tool is designed to read and write the slot and rootfs state of the Orb."
)]
#[allow(missing_docs)]
pub struct Cli {
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
    SetNextSlot { slot: String },
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
    SetRootfsStatus { status: String },
    /// Get a full list of rootfs status variants.
    #[command(name = "list", short_flag = 'l')]
    ListStatusVariants,
}

fn check_running_as_root(error: crate::Error) {
    let uid = rustix::process::getuid();
    let euid = rustix::process::geteuid();
    if !(uid.is_root() && euid.is_root()) {
        println!("Please try again as root user.");
        exit(1)
    }
    panic!("{}", error)
}

pub fn run(orb_slot_ctrl: &OrbSlotCtrl, cli: Cli) -> eyre::Result<()> {
    match cli.subcmd {
        Commands::GetSlot => {
            println!("{}", orb_slot_ctrl.get_current_slot()?);
        }
        Commands::GetNextSlot => {
            println!("{}", orb_slot_ctrl.get_next_boot_slot()?);
        }
        Commands::SetNextSlot { slot } => {
            let slot = match slot.to_lowercase().as_str() {
                // Slot A alias.
                "a" | "0" => crate::Slot::A,
                // Slot B alias.
                "b" | "1" => crate::Slot::B,
                _ => {
                    println!(
                        "Invalid slot provided, please use either A/a/0 or B/b/1."
                    );
                    exit(1)
                }
            };
            if let Err(e) = orb_slot_ctrl.set_next_boot_slot(slot) {
                check_running_as_root(e);
            };
        }
        Commands::Status { inactive, subcmd } => {
            match subcmd {
                StatusCommands::GetRootfsStatus => {
                    if inactive {
                        println!(
                            "{:?}",
                            orb_slot_ctrl.get_rootfs_status(
                                orb_slot_ctrl.get_inactive_slot()?
                            )?
                        );
                    } else {
                        println!("{:?}", orb_slot_ctrl.get_current_rootfs_status()?);
                    }
                }
                StatusCommands::SetRootfsStatus { status } => {
                    let status = match status.to_lowercase().as_str() {
                        // Status Normal alias.
                        "normal" | "0" => crate::RootFsStatus::Normal,
                        // Status UpdateInProcess alias.
                        "updateinprocess" | "updinprocess" | "1" => {
                            crate::RootFsStatus::UpdateInProcess
                        }
                        // Status UpdateDone alias.
                        "updatedone" | "upddone" | "2" => {
                            crate::RootFsStatus::UpdateDone
                        }
                        // Status Unbootable alias.
                        "unbootable" | "3" => crate::RootFsStatus::Unbootable,
                        _ => {
                            println!("Invalid status provided. For a full list of available rootfs status run:");
                            println!("slot-ctrl status --list");
                            exit(1)
                        }
                    };
                    if inactive {
                        if let Err(e) = orb_slot_ctrl.set_rootfs_status(
                            status,
                            orb_slot_ctrl.get_inactive_slot()?,
                        ) {
                            check_running_as_root(e);
                        }
                    } else if let Err(e) =
                        orb_slot_ctrl.set_current_rootfs_status(status)
                    {
                        check_running_as_root(e);
                    }
                }
                StatusCommands::ListStatusVariants => {
                    println!("Available Rootfs status variants with their aliases):");
                    println!("  Normal (normal, 0)");
                    println!("  UpdateInProcess (updateinprocess, updinprocess, 1)");
                    println!("  UpdateDone (updatedone, upddone, 2)");
                    println!("  Unbootable (unbootable, 3)");
                }
            }
        }
        Commands::GitDescribe => {
            println!("{}", BUILD_INFO.git.describe);
        }
    }

    Ok(())
}
