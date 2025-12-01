//! Debug board (FTDI FT4232H) device operations.

use camino::Utf8PathBuf;
use clap::{Parser, Subcommand};
use color_eyre::{
    eyre::{bail, ensure, eyre, WrapErr as _},
    Result,
};
use libftd2xx::{Eeprom4232h, EepromStrings, Ft4232h, Ftdi, FtdiEeprom};
use nusb::MaybeFuture;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::ftdi::{detach_all_ftdi_kernel_drivers, strip_channel_suffix, FtdiGpio};

/// Debug board (FTDI FT4232H) operations
#[derive(Debug, Parser)]
pub struct DebugBoardCmd {
    #[command(subcommand)]
    command: DebugBoardSubcommand,
}

#[derive(Debug, Subcommand)]
enum DebugBoardSubcommand {
    /// List all connected FTDI devices/channels
    List(ListCmd),
    /// Read EEPROM content and dump to a file
    Read(ReadCmd),
    /// Write EEPROM content from a file
    Write(WriteCmd),
}

/// List all connected FTDI devices
#[derive(Debug, Parser)]
struct ListCmd;

/// Read EEPROM content to a file or stdout
#[derive(Debug, Parser)]
struct ReadCmd {
    /// Output file path (JSON format). If not specified, prints to stdout.
    #[arg(long, short)]
    file: Option<Utf8PathBuf>,

    /// The USB serial number of the FTDI chip (e.g., "FT7ABC12").
    /// Note: EEPROM is shared across all channels of a chip, so any channel
    /// works for reading/writing EEPROM.
    #[arg(long, conflicts_with = "desc")]
    usb_serial: Option<String>,

    /// The FTDI description (e.g., "FT4232H A").
    /// Any channel description works since EEPROM is chip-wide.
    #[arg(long, conflicts_with = "usb_serial")]
    desc: Option<String>,
}

/// Write EEPROM content from a file
#[derive(Debug, Parser)]
struct WriteCmd {
    /// Input file path (JSON format)
    input: Utf8PathBuf,

    /// The USB serial number of the FTDI chip (e.g., "FT7ABC12").
    /// Note: EEPROM is shared across all channels of a chip, so any channel
    /// works for reading/writing EEPROM.
    #[arg(long, conflicts_with = "desc")]
    usb_serial: Option<String>,

    /// The FTDI description (e.g., "FT4232H A").
    /// Any channel description works since EEPROM is chip-wide.
    #[arg(long, conflicts_with = "usb_serial")]
    desc: Option<String>,
}

/// Serializable representation of EEPROM data for FT4232H.
#[derive(Debug, Serialize, Deserialize)]
struct Ft4232hEepromData {
    /// Vendor ID (typically 0x0403 for FTDI)
    vendor_id: u16,
    /// Product ID (typically 0x6011 for FT4232H)
    product_id: u16,
    /// Whether the serial number is enabled
    serial_number_enable: bool,
    /// Maximum bus current in milliamps (0-500)
    max_current_ma: u16,
    /// Self-powered device
    self_powered: bool,
    /// Remote wakeup capable
    remote_wakeup: bool,
    /// Pull-down in suspend enabled
    pull_down_enable: bool,

    // String fields
    /// Manufacturer string
    manufacturer: String,
    /// Manufacturer ID
    manufacturer_id: String,
    /// Product description
    description: String,
    /// Serial number
    serial_number: String,
}

impl Ft4232hEepromData {
    fn from_eeprom(eeprom: &Eeprom4232h, strings: &EepromStrings) -> Self {
        let header = eeprom.header();
        Self {
            vendor_id: header.vendor_id(),
            product_id: header.product_id(),
            serial_number_enable: header.serial_number_enable(),
            max_current_ma: header.max_current(),
            self_powered: header.self_powered(),
            remote_wakeup: header.remote_wakeup(),
            pull_down_enable: header.pull_down_enable(),

            manufacturer: strings.manufacturer(),
            manufacturer_id: strings.manufacturer_id(),
            description: strings.description(),
            serial_number: strings.serial_number(),
        }
    }

    fn to_eeprom(&self) -> Result<(Eeprom4232h, EepromStrings)> {
        let mut eeprom = Eeprom4232h::default();
        let mut header = eeprom.header();

        header.set_device_type(libftd2xx::DeviceType::FT4232H);
        header.set_vendor_id(self.vendor_id);
        header.set_product_id(self.product_id);
        header.set_serial_number_enable(self.serial_number_enable);
        header.set_max_current(self.max_current_ma);
        header.set_self_powered(self.self_powered);
        header.set_remote_wakeup(self.remote_wakeup);
        header.set_pull_down_enable(self.pull_down_enable);
        eeprom.set_header(header);

        let strings = EepromStrings::with_strs(
            &self.manufacturer,
            &self.manufacturer_id,
            &self.description,
            &self.serial_number,
        )
        .map_err(|e| eyre!("EEPROM strings error: {e:?}"))?;

        Ok((eeprom, strings))
    }
}

impl DebugBoardCmd {
    pub async fn run(self) -> Result<()> {
        match self.command {
            DebugBoardSubcommand::List(cmd) => cmd.run().await,
            DebugBoardSubcommand::Read(cmd) => cmd.run().await,
            DebugBoardSubcommand::Write(cmd) => cmd.run().await,
        }
    }
}

impl ListCmd {
    async fn run(self) -> Result<()> {
        tokio::task::spawn_blocking(|| -> Result<()> {
            detach_all_ftdi_kernel_drivers();

            let devices: Vec<_> = FtdiGpio::list_devices()
                .wrap_err("failed to list FTDI devices")?
                .collect();

            if devices.is_empty() {
                println!("No FTDI devices found.");
                return Ok(());
            }

            // Group devices by USB serial (strip channel suffix from FTDI serial)
            let mut grouped: std::collections::BTreeMap<String, Vec<_>> =
                std::collections::BTreeMap::new();
            for device in &devices {
                let usb_serial =
                    strip_channel_suffix(&device.serial_number).to_string();
                grouped.entry(usb_serial).or_default().push(device);
            }

            let chip_count = grouped.len();
            let channel_count = devices.len();
            println!(
                "Found {chip_count} debug board(s) ({channel_count} channel(s) total):\n",
            );

            for (i, (usb_serial, channels)) in grouped.iter().enumerate() {
                let first = channels.first().unwrap();

                println!("Chip {} [USB Serial: {}]:", i + 1, usb_serial);
                println!("  Vendor ID:    0x{:04X}", first.vendor_id);
                println!("  Product ID:   0x{:04X}", first.product_id);
                println!("  Channels:");

                for channel in channels {
                    // Extract just the channel letter from description (e.g., "FT4232H A" -> "A")
                    let channel_letter =
                        channel.description.chars().last().unwrap_or('?');
                    println!(
                        "    {}: FTDI Serial = {}, Description = \"{}\"",
                        channel_letter, channel.serial_number, channel.description
                    );
                }
                println!();
            }

            Ok(())
        })
        .await
        .wrap_err("task panicked")?
    }
}

impl ReadCmd {
    async fn run(self) -> Result<()> {
        let output_path = self.file.clone();
        let usb_serial = self.usb_serial.clone();
        let desc = self.desc.clone();

        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut ft4232h = open_ft4232h(usb_serial.as_deref(), desc.as_deref())?;

            info!("Reading EEPROM from FT4232H device...");
            let (eeprom, strings) = ft4232h
                .eeprom_read()
                .map_err(|e| eyre!("failed to read EEPROM: {e:?}"))?;

            let data = Ft4232hEepromData::from_eeprom(&eeprom, &strings);
            debug!("EEPROM data: {data:?}");

            let json = serde_json::to_string_pretty(&data)
                .wrap_err("failed to serialize EEPROM data to JSON")?;

            if let Some(output_path) = output_path {
                std::fs::write(&output_path, &json)
                    .wrap_err_with(|| format!("failed to write to {output_path}"))?;
                info!("EEPROM content written to {output_path}");
            } else {
                println!("{json}");
            }

            Ok(())
        })
        .await
        .wrap_err("task panicked")?
    }
}

impl WriteCmd {
    async fn run(self) -> Result<()> {
        let input_path = self.input.clone();
        let usb_serial = self.usb_serial.clone();
        let desc = self.desc.clone();

        tokio::task::spawn_blocking(move || -> Result<()> {
            let json = std::fs::read_to_string(&input_path)
                .wrap_err_with(|| format!("failed to read {input_path}"))?;

            let data: Ft4232hEepromData = serde_json::from_str(&json)
                .wrap_err("failed to parse EEPROM data from JSON")?;

            info!("Writing EEPROM to FT4232H device...");
            info!("  Serial number: {}", data.serial_number);
            info!("  Description: {}", data.description);
            info!("  Manufacturer: {}", data.manufacturer);

            let (eeprom, strings) = data.to_eeprom()?;

            let mut ft4232h = open_ft4232h(usb_serial.as_deref(), desc.as_deref())?;

            ft4232h
                .eeprom_program(eeprom, strings)
                .map_err(|e| eyre!("failed to program EEPROM: {e:?}"))?;

            info!("EEPROM successfully programmed!");
            info!("Note: You may need to unplug and replug the device for changes to take effect.");

            Ok(())
        })
        .await
        .wrap_err("task panicked")?
    }
}

/// Opens an FT4232H device with optional USB serial or description filter.
///
/// Note: For USB serial, we append 'A' to get the FTDI serial of the first channel,
/// since EEPROM operations work the same on any channel of the same chip.
fn open_ft4232h(usb_serial: Option<&str>, desc: Option<&str>) -> Result<Ft4232h> {
    detach_all_ftdi_kernel_drivers();

    match (usb_serial, desc) {
        (Some(usb_serial), None) => open_ft4232h_by_usb_serial(usb_serial),
        (None, Some(desc)) => open_ft4232h_by_description(desc),
        (None, None) => open_default_ft4232h(),
        (Some(_), Some(_)) => {
            bail!("cannot specify both USB serial and description")
        }
    }
}

fn open_default_ft4232h() -> Result<Ft4232h> {
    let usb_device_infos: Vec<_> = nusb::list_devices()
        .wait()
        .wrap_err("failed to enumerate devices")?
        .filter(|d| d.vendor_id() == libftd2xx::FTDI_VID)
        .collect();

    if usb_device_infos.is_empty() {
        bail!("no FTDI devices found");
    }
    if usb_device_infos.len() > 1 {
        bail!(
            "multiple FTDI devices found, please specify --usb-serial or --desc to select one"
        );
    }

    let usb_device_info = usb_device_infos.into_iter().next().unwrap();

    // Detach kernel drivers if needed
    if let Ok(usb_device) = usb_device_info.open().wait() {
        for iinfo in usb_device_info.interfaces() {
            let _ = usb_device.detach_kernel_driver(iinfo.interface_number());
        }
    }

    let ftdi = Ftdi::new().wrap_err("failed to open FTDI device")?;
    let ft4232h: Ft4232h = ftdi
        .try_into()
        .map_err(|e| eyre!("device is not an FT4232H: {e:?}"))?;

    Ok(ft4232h)
}

fn open_ft4232h_by_usb_serial(usb_serial: &str) -> Result<Ft4232h> {
    ensure!(!usb_serial.is_empty(), "USB serial cannot be empty");

    let usb_device_info = nusb::list_devices()
        .wait()
        .wrap_err("failed to enumerate devices")?
        .find(|d| d.serial_number() == Some(usb_serial))
        .ok_or_else(|| eyre!("no device with USB serial \"{usb_serial}\" found"))?;

    // Detach kernel drivers if needed
    if let Ok(usb_device) = usb_device_info.open().wait() {
        for iinfo in usb_device_info.interfaces() {
            let _ = usb_device.detach_kernel_driver(iinfo.interface_number());
        }
    }

    // Use channel A (append 'A') to get the FTDI serial.
    // EEPROM is shared across all channels, so any channel works.
    let ftdi_serial = format!("{usb_serial}A");
    let ftdi = Ftdi::with_serial_number(&ftdi_serial).map_err(|e| {
        eyre!("failed to open FTDI device with FTDI serial \"{ftdi_serial}\": {e:?}")
    })?;
    let ft4232h: Ft4232h = ftdi
        .try_into()
        .map_err(|e| eyre!("device is not an FT4232H: {e:?}"))?;

    Ok(ft4232h)
}

fn open_ft4232h_by_description(desc: &str) -> Result<Ft4232h> {
    let usb_device_infos: Vec<_> = nusb::list_devices()
        .wait()
        .wrap_err("failed to enumerate devices")?
        .filter(|d| d.vendor_id() == libftd2xx::FTDI_VID)
        .collect();

    // Detach kernel drivers for all FTDI devices
    for usb_device_info in &usb_device_infos {
        if let Ok(usb_device) = usb_device_info.open().wait() {
            for iinfo in usb_device_info.interfaces() {
                let _ = usb_device.detach_kernel_driver(iinfo.interface_number());
            }
        }
    }

    let ftdi = Ftdi::with_description(desc).map_err(|e| {
        eyre!("failed to open FTDI device with description \"{desc}\": {e:?}")
    })?;
    let ft4232h: Ft4232h = ftdi
        .try_into()
        .map_err(|e| eyre!("device is not an FT4232H: {e:?}"))?;

    Ok(ft4232h)
}
