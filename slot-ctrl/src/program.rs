use crate::{BootChainFwStatus, Error, OrbSlotCtrl, RootFsStatus, Slot};
use clap::{Parser, Subcommand};
use color_eyre::{eyre::bail, Result};
use orb_build_info::{make_build_info, BuildInfo};
use orb_info::orb_os_release::OrbType;
use std::{env, str::FromStr};

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

    /// Bootchain firmware status controls.
    BootchainFw {
        #[command(subcommand)]
        subcmd: BootchainFwCommands,
    },

    /// Get the git commit used for this build.
    #[command(name = "git", short_flag = 'g')]
    GitDescribe,
}

#[derive(Subcommand)]
enum BootchainFwCommands {
    /// Get the boot chain firmware status.
    #[command(name = "get")]
    Get,
    /// Set the boot chain firmware status.
    #[command(name = "set")]
    Set { status: u8 },

    #[command(name = "delete")]
    Delete,
}

#[derive(Subcommand)]
enum StatusCommands {
    /// Get the rootfs status.
    #[command(name = "get", short_flag = 'g')]
    GetRootfsStatus,
    /// Set the rootfs status.
    #[command(name = "set", short_flag = 's')]
    SetRootfsStatus { status: String },
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

fn check_running_as_root(e: Error) -> Result<()> {
    let uid = rustix::process::getuid();
    let euid = rustix::process::geteuid();
    if !(uid.is_root() && euid.is_root()) {
        bail!("Please try again as root user. Error: {e}");
    }

    Ok(())
}

pub fn run(slot_ctrl: &OrbSlotCtrl, cli: Cli) -> Result<String> {
    let empty = String::new();
    let output = match cli.subcmd {
        Commands::GetSlot => slot_ctrl.get_current_slot()?.to_string(),

        Commands::GetNextSlot => slot_ctrl.get_next_boot_slot()?.to_string(),

        Commands::SetNextSlot { slot } => {
            let slot = Slot::from_str(&slot)?;
            if let Err(e) = slot_ctrl.set_next_boot_slot(slot) {
                check_running_as_root(e)?;
            }

            empty
        }

        Commands::BootchainFw { subcmd } => match subcmd {
            BootchainFwCommands::Get => {
                slot_ctrl.read_bootchain_fw_status()?.to_string()
            }

            BootchainFwCommands::Set { status } => {
                let status = BootChainFwStatus::try_from(status)?;
                if let Err(e) = slot_ctrl.set_bootchain_fw_status(status) {
                    check_running_as_root(e)?;
                }

                empty
            }

            BootchainFwCommands::Delete => {
                if let Err(e) = slot_ctrl.delete_bootchain_fw_status() {
                    check_running_as_root(e)?;
                    empty
                } else {
                    String::from("Successfully deleted BootchainFwStatus EfiVar")
                }
            }
        },

        Commands::Status { inactive, subcmd } => {
            let slot = if inactive {
                slot_ctrl.get_inactive_slot()?
            } else {
                slot_ctrl.get_current_slot()?
            };

            match subcmd {
                StatusCommands::GetRootfsStatus => {
                    slot_ctrl.get_rootfs_status(slot)?.to_string()
                }

                StatusCommands::SetRootfsStatus { status } => {
                    let status = RootFsStatus::from_str(&status)?;
                    if let Err(e) = slot_ctrl.set_rootfs_status(status, slot) {
                        check_running_as_root(e)?;
                    }

                    empty
                }

                StatusCommands::GetRetryCounter => {
                    slot_ctrl.get_retry_count(slot)?.to_string()
                }

                StatusCommands::GetMaxRetryCounter => {
                    slot_ctrl.get_max_retry_count()?.to_string()
                }

                StatusCommands::ResetRetryCounter => {
                    if let Err(e) = slot_ctrl.reset_retry_count_to_max(slot) {
                        check_running_as_root(e)?;
                    }

                    empty
                }

                StatusCommands::ListStatusVariants => {
                    let mut output = String::new();
                    output.push_str(
                        "Available Rootfs status variants with their aliases):\n",
                    );
                    output.push_str("  Normal (normal, 0)\n");
                    if OrbType::Pearl == slot_ctrl.orb_type {
                        output.push_str(
                            "  UpdateInProcess (updateinprocess, updinprocess, 1)\n",
                        );
                        output.push_str("  UpdateDone (updatedone, upddone, 2)\n");
                    }
                    output.push_str("  Unbootable (unbootable, 3)\n");

                    output
                }
            }
        }

        Commands::GitDescribe => BUILD_INFO.git.describe.to_string(),
    };

    Ok(output)
}
