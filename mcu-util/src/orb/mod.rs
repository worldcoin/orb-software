#![allow(clippy::uninlined_format_args)]
use std::fmt::{Display, Formatter};
use std::time::Duration;

use crate::orb::main_board::MainBoard;
use crate::orb::revision::OrbRevision;
use crate::orb::security_board::SecurityBoard;
use async_trait::async_trait;
use color_eyre::eyre::{eyre, Context, Result};
use futures::FutureExt;
use orb_mcu_interface::can::CanTaskHandle;
use orb_mcu_interface::orb_messages::hardware_state::Status;
use orb_mcu_interface::orb_messages::main as main_messaging;
use orb_mcu_interface::orb_messages::CommonAckError;
use orb_mcu_interface::{orb_messages, McuPayload};
use tracing::info;

mod dfu;
pub mod main_board;
mod revision;
pub mod security_board;

#[async_trait]
pub trait Board {
    /// Reboot the board.
    async fn reboot(&mut self, delay: Option<u32>) -> Result<()>;

    /// Fetch the board information and update the `info` struct
    async fn fetch_info(&mut self, info: &mut OrbInfo, diag: bool) -> Result<()>;

    /// Print out all the messages received from the board for the given `duration`.
    /// If no duration is provided, the function will print out all the messages
    /// indefinitely.
    /// If `logs_only` is set to `true`, only the logs (errors and warnings) will be printed.
    async fn dump(&mut self, duration: Option<Duration>, logs_only: bool)
        -> Result<()>;

    /// Send a new firmware image to the board
    /// This operation will also switch the board, and in case
    /// of the security microcontroller, it will reboot the board
    /// to perform the update.
    ///
    /// If `force` is false, the update will be skipped if the binary version
    /// matches the currently running firmware version on the MCU.
    async fn update_firmware(&mut self, path: &str, force: bool) -> Result<()>;

    /// Switch the firmware images on the board, from secondary to primary
    /// Images are checked for validity before the switch: if the images are
    /// not valid or not compatible (ie. a dev image on a prod bootloader),
    /// the switch will not be performed. Use the `force` flag to bypass checks.
    async fn switch_images(&mut self, force: bool) -> Result<()>;

    /// Stress test the board for the given duration
    /// Communication across the different channels (CAN-FD, ISO-TP & UART)
    /// is performed as fast as possible to stress the microcontroller
    /// Statistic messages are printed every second
    async fn stress_test(&mut self, duration: Option<Duration>) -> Result<()>;
}

pub struct Orb {
    main_board: MainBoard,
    sec_board: SecurityBoard,
    info: OrbInfo,
}

impl Orb {
    pub async fn new(can_fd: bool) -> Result<(Self, OrbTaskHandles)> {
        let (main_board, main_task_handle) = MainBoard::builder().build(can_fd).await?;
        let (sec_board, sec_task_handle) =
            SecurityBoard::builder().build(can_fd).await?;
        let info = OrbInfo::default();

        Ok((
            Self {
                main_board,
                sec_board,
                info,
            },
            OrbTaskHandles {
                main: main_task_handle,
                sec: sec_task_handle,
            },
        ))
    }

    pub fn board_mut(&mut self, mcu: crate::Mcu) -> &mut dyn Board {
        match mcu {
            crate::Mcu::Main => &mut self.main_board,
            crate::Mcu::Security => &mut self.sec_board,
        }
    }

    pub fn main_board_mut(&mut self) -> &mut MainBoard {
        &mut self.main_board
    }

    pub fn sec_board_mut(&mut self) -> &mut SecurityBoard {
        &mut self.sec_board
    }

    pub async fn get_info(&mut self, diag: bool) -> Result<OrbInfo> {
        self.main_board.fetch_info(&mut self.info, diag).await?;
        self.sec_board.fetch_info(&mut self.info, diag).await?;
        Ok(self.info.clone())
    }

    pub async fn get_revision(&mut self) -> Result<OrbRevision> {
        self.main_board.fetch_info(&mut self.info, false).await?;
        Ok(self.info.hw_rev.clone().unwrap_or_default())
    }

    pub async fn reboot(&mut self, delay: Option<u32>) -> Result<()> {
        let reboot_orb_msg =
            McuPayload::ToMain(main_messaging::jetson_to_mcu::Payload::RebootOrb(
                main_messaging::RebootOrb {
                    force_reboot_timeout_s: delay
                        .unwrap_or(0 /* wait for jetson's graceful shutdown */),
                },
            ));
        match self.main_board.send(reboot_orb_msg).await {
            Ok(CommonAckError::Success) => {
                if delay.is_some() {
                    info!("🚦 The Orb will be forced to reboot in {} seconds. Better to gracefully shutdown with `sudo shutdown now`", delay.unwrap());
                } else {
                    info!("🚦 The Orb will reboot once you shutdown the Jetson gracefully: `sudo shutdown now`");
                }
            }
            Ok(e) => {
                return Err(eyre!("Error rebooting the orb: ack error: {:?}", e));
            }
            Err(e) => {
                return Err(eyre!("Error rebooting the orb: {:?}", e));
            }
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
pub struct OrbInfo {
    pub hw_rev: Option<OrbRevision>,
    pub main_fw_versions: Option<orb_messages::Versions>,
    pub sec_fw_versions: Option<orb_messages::Versions>,
    pub main_battery_status: Option<BatteryStatus>,
    pub sec_battery_status: Option<BatteryStatus>,
    pub hardware_states: Vec<orb_messages::HardwareState>,
}

#[derive(Clone, Debug)]
pub struct BatteryStatus {
    percentage: Option<u32>,
    voltage_mv: Option<u32>,
    is_charging: Option<bool>,
    is_corded: Option<bool>,
}

impl Display for OrbInfo {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        // pretty printing
        write!(f, "{}Orb info:\r\n", if f.alternate() { "🔮 " } else { "" },)?;
        if let Some(hw) = self.hw_rev.clone() {
            write!(f, "\trevision:\t{}\r\n", hw)?;
        }
        if let Some(battery) = self.main_battery_status.clone() {
            if let Some(is_corded) = battery.is_corded {
                write!(
                    f,
                    "\tpower supply:\t{}\r\n",
                    if is_corded {
                        "corded 🔌"
                    } else {
                        "battery 🔋"
                    }
                )?;
            }
            if let Some(voltage) = battery.voltage_mv {
                write!(f, "\tvoltage:\t{}mV\r\n", voltage)?;
            }
            if let Some(false) = battery.is_corded {
                if let Some(capacity) = battery.percentage {
                    write!(f, "\tbattery charge:\t{}%\r\n", capacity)?;
                }
                if let Some(is_charging) = battery.is_charging {
                    write!(
                        f,
                        "\tcharging:\t{}\r\n",
                        if is_charging { "yes" } else { "no" }
                    )?;
                }
            }
        } else {
            write!(f, "\tbattery:\tunknown\r\n")?;
        }

        // print main board info
        write!(
            f,
            "{}Main board:\r\n",
            if f.alternate() { "🚜 " } else { "" },
        )?;
        if let Some(main) = self.main_fw_versions.clone() {
            if let Some(primary) = main.primary_app {
                write!(
                    f,
                    "\tcurrent image:\tv{}.{}.{}-0x{:x}{}\r\n",
                    primary.major,
                    primary.minor,
                    primary.patch,
                    primary.commit_hash,
                    if primary.commit_hash == 0 {
                        " (dev)"
                    } else {
                        " (prod)"
                    }
                )?;
                if let Some(secondary) = main.secondary_app {
                    write!(f, "\tsecondary slot:\t")?;
                    if secondary.major != 255
                        && secondary.minor != 255
                        && secondary.patch != 255
                    {
                        write!(
                            f,
                            "v{}.{}.{}-0x{:x}{}\r\n",
                            secondary.major,
                            secondary.minor,
                            secondary.patch,
                            secondary.commit_hash,
                            if secondary.commit_hash == 0 {
                                " (dev)"
                            } else {
                                " (prod)"
                            }
                        )?;
                    } else {
                        write!(f, "unused?\r\n")?;
                    }
                }
            }
        } else {
            write!(f, "\tfirmware image:\tunknown state\r\n")?;
        }

        // print security board info
        write!(
            f,
            "{}Security board:\r\n",
            if f.alternate() { "🔐 " } else { "" },
        )?;
        if let Some(sec) = self.sec_fw_versions.clone() {
            if let Some(primary) = sec.primary_app {
                write!(
                    f,
                    "\tcurrent image:\tv{}.{}.{}-0x{:x}{}\r\n",
                    primary.major,
                    primary.minor,
                    primary.patch,
                    primary.commit_hash,
                    if primary.commit_hash == 0 {
                        " (dev)"
                    } else {
                        " (prod)"
                    }
                )?;
                if let Some(secondary) = sec.secondary_app {
                    write!(f, "\tsecondary slot:\t")?;
                    if secondary.major != 255
                        && secondary.minor != 255
                        && secondary.patch != 255
                    {
                        write!(
                            f,
                            "v{}.{}.{}-0x{:x}{}\r\n",
                            secondary.major,
                            secondary.minor,
                            secondary.patch,
                            secondary.commit_hash,
                            if secondary.commit_hash == 0 {
                                " (dev)"
                            } else {
                                " (prod)"
                            }
                        )?;
                    } else {
                        write!(f, "unused?\r\n")?;
                    }
                }
            }
        } else {
            write!(f, "\tfirmware image:\tunknown\r\n")?;
        }

        if let Some(battery) = self.sec_battery_status.clone() {
            if let Some(capacity) = battery.percentage {
                write!(f, "\tbattery charge:\t{}%\r\n", capacity)?;
            }
            if let Some(voltage) = battery.voltage_mv {
                write!(f, "\tvoltage:\t{}mV\r\n", voltage)?;
            }
            if let Some(is_charging) = battery.is_charging {
                write!(
                    f,
                    "\tcharging:\t{}\r\n",
                    if is_charging { "yes" } else { "no" }
                )?;
            }
        } else {
            write!(f, "\tbackup battery:\tunknown\r\n")?;
        }

        if !self.hardware_states.is_empty() {
            write!(f, "\r\n🛠️ Hardware states:\r\n")?;
            for state in &self.hardware_states {
                write!(
                    f,
                    "{:<12} {:<35} {}\r\n",
                    state.source_name,
                    Status::try_from(state.status)
                        .unwrap_or_default()
                        .as_str_name(),
                    if state.message.is_empty() {
                        ""
                    } else {
                        &state.message
                    }
                )?;
            }
        }

        Ok(())
    }
}

#[derive(Debug)]
pub struct BoardTaskHandles {
    pub raw: CanTaskHandle,
    pub isotp: Option<CanTaskHandle>,
}

impl BoardTaskHandles {
    pub async fn join(self) -> color_eyre::Result<()> {
        self.raw
            .map(|e| e.wrap_err("raw can task terminated"))
            .await?;
        if let Some(isotp) = self.isotp {
            isotp
                .map(|e| e.wrap_err("isotp can task terminated"))
                .await?;
        }

        Ok(())
    }
}

#[derive(Debug)]
pub struct OrbTaskHandles {
    pub main: BoardTaskHandles,
    pub sec: BoardTaskHandles,
}

impl OrbTaskHandles {
    pub async fn join(self) -> color_eyre::Result<()> {
        let ((), ()) = tokio::try_join!(
            self.main
                .join()
                .map(|e| e.wrap_err("main board task(s) terminated")),
            self.sec
                .join()
                .map(|e| e.wrap_err("sec board task(s) terminated"))
        )?;
        Ok(())
    }
}
