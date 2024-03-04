use std::fmt::{Display, Formatter};
use std::time::Duration;

use async_trait::async_trait;
use eyre::Result;
use orb_mcu_messaging::mcu_main as main_messaging;
use orb_mcu_messaging::mcu_sec as sec_messaging;

use crate::orb::main_board::MainBoard;
use crate::orb::revision::OrbRevision;
use crate::orb::security_board::SecurityBoard;

mod dfu;
pub mod main_board;
mod revision;
pub mod security_board;

#[async_trait]
pub trait Board {
    /// Reboot the board.
    async fn reboot(&mut self, delay: Option<u32>) -> Result<()>;

    /// Fetch the board information and update the `info` struct
    async fn fetch_info(&mut self, info: &mut OrbInfo) -> Result<()>;

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
    async fn update_firmware(&mut self, path: &str, canfd: bool) -> Result<()>;

    /// Switch the firmware images on the board, from secondary to primary
    /// Images are checked for validity before the switch: if the images are
    /// not valid or not compatible (ie. a dev image on a prod bootloader),
    /// the switch will not be performed.
    async fn switch_images(&mut self) -> Result<()>;

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
    pub async fn new() -> Result<Self> {
        let main_board = MainBoard::builder().build().await?;
        let sec_board = SecurityBoard::builder().build().await?;
        let info = OrbInfo::default();

        Ok(Self {
            main_board,
            sec_board,
            info,
        })
    }

    pub fn borrow_mut_mcu(&mut self, mcu: crate::Mcu) -> &mut dyn Board {
        match mcu {
            crate::Mcu::Main => &mut self.main_board,
            crate::Mcu::Security => &mut self.sec_board,
        }
    }

    pub fn borrow_mut_sec_board(&mut self) -> &mut SecurityBoard {
        &mut self.sec_board
    }

    pub async fn get_info(&mut self) -> Result<OrbInfo> {
        self.main_board.fetch_info(&mut self.info).await?;
        self.sec_board.fetch_info(&mut self.info).await?;
        Ok(self.info.clone())
    }

    pub async fn get_revision(&mut self) -> Result<OrbRevision> {
        self.main_board.fetch_info(&mut self.info).await?;
        Ok(self.info.hw_rev.clone().unwrap_or_default())
    }
}

#[derive(Clone, Debug, Default)]
pub struct OrbInfo {
    pub hw_rev: Option<OrbRevision>,
    pub main_fw_versions: Option<main_messaging::Versions>,
    pub sec_fw_versions: Option<sec_messaging::Versions>,
    pub main_battery_status: Option<BatteryStatus>,
    pub sec_battery_status: Option<BatteryStatus>,
}

#[derive(Clone, Debug)]
pub struct BatteryStatus {
    percentage: Option<u32>,
    voltage_mv: Option<u32>,
    is_charging: Option<bool>,
}

impl Display for OrbInfo {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        // pretty printing
        write!(f, "{}Orb info:\r\n", if f.alternate() { "🔮 " } else { "" },)?;
        if let Some(hw) = self.hw_rev.clone() {
            write!(f, "\trevision:\t{}\r\n", hw)?;
        }
        if let Some(battery) = self.main_battery_status.clone() {
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

        Ok(())
    }
}
