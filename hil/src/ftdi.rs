//! Code to control GPIO of FTDI serial adapter
//!
//! # Notes
//!
//! ## Missing EEPROM serial numbers
//! Sometimes, a FTDI device has no serial number. It may show up as an empty
//! string when using libftd2xx, or as "000000000" in lsusb and nusb.
//!
//! When this happens, its typically because the chip has no EEPROM attached,
//! or the EEPROM is blank/unprogrammed. While it is still desirable to set the
//! EEPROM to be able to unambiguously identity a particular chip, this code makes
//! affordances for situations where this is not the case, however only at most
//! one such blank FTDI device can be attached at any given time.
//!
//! Read more in section 4.2 of
//! <https://ftdichip.com/wp-content/uploads/2024/09/DS_FT4232H.pdf>

use color_eyre::{
    eyre::{bail, ensure, eyre, OptionExt, WrapErr as _},
    Result,
};
use libftd2xx::FtdiCommon;
use nusb::MaybeFuture;

/// Whether the pin is being pulled high or low.
#[derive(Debug, Eq, Hash, PartialEq, Copy, Clone)]
pub enum OutputState {
    High = 1,
    Low = 0,
}

/// Newtype for pins of the FTDI adapter.
#[derive(Debug, Eq, PartialEq, Hash, Clone, Copy)]
pub struct Pin(u8);

/// States used for the [`Builder`].
mod builder_states {
    /// The device needs to be specified.
    #[derive(Debug, Clone)]
    pub struct NeedsDevice;

    /// The device has been provided but we still need to configure the device.
    #[derive(Debug)]
    pub struct NeedsConfiguring {
        pub device: libftd2xx::Ftdi,
    }
}
use builder_states::*;
use tracing::{debug, error, warn};

/// The 4 channels of an FT4232H chip.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum FtdiChannel {
    A,
    B,
    C,
    D,
}

impl FtdiChannel {
    pub fn as_char(self) -> char {
        match self {
            FtdiChannel::A => 'A',
            FtdiChannel::B => 'B',
            FtdiChannel::C => 'C',
            FtdiChannel::D => 'D',
        }
    }

    pub fn description_suffix(self) -> &'static str {
        match self {
            FtdiChannel::A => "FT4232H A",
            FtdiChannel::B => "FT4232H B",
            FtdiChannel::C => "FT4232H C",
            FtdiChannel::D => "FT4232H D",
        }
    }
}

impl std::str::FromStr for FtdiChannel {
    type Err = color_eyre::Report;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_uppercase().as_str() {
            "A" => Ok(FtdiChannel::A),
            "B" => Ok(FtdiChannel::B),
            "C" => Ok(FtdiChannel::C),
            "D" => Ok(FtdiChannel::D),
            _ => Err(color_eyre::eyre::eyre!(
                "invalid channel: {s}, expected A, B, C, or D"
            )),
        }
    }
}

/// The different supported ways to address a *specific* FTDI device/channel.
///
/// # Terminology
/// - **USB serial**: The physical USB device serial number (what `nusb`/`lsusb` sees).
///   One FT4232H chip = one USB serial (e.g., "FT7ABC12").
/// - **FTDI serial**: The channel-specific serial, which is `{usb_serial}{channel}`.
///   The FTDI library (libftd2xx) sees 4 "devices" per FT4232H: A, B, C, D
///   (e.g., "FT7ABC12A", "FT7ABC12B", "FT7ABC12C", "FT7ABC12D").
/// - **Description**: The FTDI description which identifies the channel type
///   (e.g., "FT4232H A", "FT4232H B", "FT4232H C", "FT4232H D").
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum FtdiId {
    /// The FTDI channel serial (USB serial + channel letter, e.g., "FT7ABC12C").
    FtdiSerial(String),
    /// The FTDI description (e.g., "FT4232H C").
    Description(String),
}

impl FtdiId {
    /// Creates an FtdiId from a USB serial number and channel.
    ///
    /// The FTDI serial is the USB serial with the channel letter appended.
    pub fn from_usb_serial(usb_serial: &str, channel: FtdiChannel) -> Self {
        Self::FtdiSerial(format!("{}{}", usb_serial, channel.as_char()))
    }
}

/// Detaches kernel drivers from all FTDI USB devices.
///
/// This is necessary because `libftd2xx` functions (like `list_devices()`) may
/// return zeroed/invalid data if kernel drivers are attached or in a bad state.
/// Call this before any libftd2xx operations that enumerate or discover devices.
pub fn detach_all_ftdi_kernel_drivers() {
    let Ok(devices) = nusb::list_devices().wait() else {
        return;
    };
    for usb_device_info in devices.filter(|d| d.vendor_id() == libftd2xx::FTDI_VID) {
        if let Ok(usb_device) = usb_device_info.open().wait() {
            for iinfo in usb_device_info.interfaces() {
                let _ = usb_device.detach_kernel_driver(iinfo.interface_number());
            }
        }
    }
}

/// Type-state builder pattern for creating a [`FtdiGpio`].
#[derive(Clone, Debug)]
pub struct Builder<S>(S);

impl Builder<NeedsDevice> {
    /// Opens the first ftdi device identified. This can change across subsequent calls,
    /// if you need a specific device use [`Self::with_ftdi_serial`] or
    /// [`Self::with_usb_serial`] instead.
    ///
    /// Returns an error if there is more than 1 FTDI device connected.
    pub fn with_default_device(self) -> Result<Builder<NeedsConfiguring>> {
        detach_all_ftdi_kernel_drivers();

        let usb_device_infos: Vec<_> = nusb::list_devices()
            .wait()
            .wrap_err("failed to enumerate devices")?
            .filter(|d| d.vendor_id() == libftd2xx::FTDI_VID)
            .collect();
        let ftdi_device_count = FtdiGpio::list_devices()
            .wrap_err("failed to enumerate ftdi devices")?
            .count();
        if usb_device_infos.is_empty() || ftdi_device_count == 0 {
            bail!("no FTDI devices found");
        }
        if usb_device_infos.len() != 1 {
            bail!("more than one FTDI USB device found");
        }
        if ftdi_device_count > 4 {
            // More than 4 FTDI channels means multiple physical chips
            bail!("more than one FTDI chip detected (more than 4 channels)");
        }
        let usb_device_info = usb_device_infos.into_iter().next_back().unwrap();

        // See module-level docs for more info about missing serial numbers.
        let usb_serial = usb_device_info.serial_number().unwrap_or("");
        if !usb_serial.is_empty() && usb_serial != "000000000" {
            // Use channel C by default when opening via USB serial
            return self.with_usb_serial(usb_serial, FtdiChannel::C);
        }

        warn!("EEPROM is either blank or missing and there is no serial number");
        let mut device =
            libftd2xx::Ftdi::new().wrap_err("failed to open default ftdi device")?;
        let device_info = device.device_info().wrap_err("failed to get device info")?;
        debug!("using device: {device_info:?}");

        Ok(Builder(NeedsConfiguring { device }))
    }

    /// Opens a device with the given FTDI serial number.
    ///
    /// The FTDI serial is the USB serial + channel letter (e.g., "FT7ABC12C").
    /// This is what `libftd2xx` uses internally.
    pub fn with_ftdi_serial(
        self,
        ftdi_serial: &str,
    ) -> Result<Builder<NeedsConfiguring>> {
        ensure!(!ftdi_serial.is_empty(), "FTDI serial cannot be empty");
        ensure!(
            ftdi_serial != "000000000",
            "FTDI serial cannot be the special zero serial"
        );

        // The USB serial is the FTDI serial without the last character in case of several
        // channels (channel letter).
        // Serial is matched to usb_serial OR ftdi_serial to ensure compatibility with
        // one-channel FTDI chips
        let usb_serial = strip_channel_suffix(ftdi_serial);

        let mut last_err = None;
        let usb_device_info = nusb::list_devices()
            .wait()
            .wrap_err("failed to enumerate devices")?
            .find(|d| {
                d.serial_number() == Some(usb_serial)
                    || d.serial_number() == Some(ftdi_serial)
            })
            .ok_or_else(|| {
                eyre!("usb device with matching serial \"{usb_serial}\" not found")
            })?;
        let usb_device = usb_device_info
            .open()
            .wait()
            .wrap_err("failed to open usb device")?;
        for iinfo in usb_device_info.interfaces() {
            // Detaching the iface from other kernel drivers is necessary for
            // libftd2xx to work.
            // See also https://stackoverflow.com/a/34021765
            let _ = usb_device.detach_kernel_driver(iinfo.interface_number());
            match libftd2xx::Ftdi::with_serial_number(ftdi_serial).wrap_err_with(|| {
                format!("failed to open FTDI device with FTDI serial \"{ftdi_serial}\"")
            }) {
                Ok(ftdi) => {
                    return Ok(Builder(NeedsConfiguring { device: ftdi }));
                }
                Err(err) => last_err = Some(err),
            }
        }
        if let Some(last_err) = last_err {
            Err(last_err).wrap_err(
                "failed to successfully open any ftdi devices. Wrapping last error",
            )
        } else {
            Err(eyre!("failed to find any ftdi devices"))
        }
    }

    /// Opens a device with the given USB serial number and channel.
    ///
    /// This is a convenience method that combines the USB serial with the channel
    /// to form the FTDI serial.
    pub fn with_usb_serial(
        self,
        usb_serial: &str,
        channel: FtdiChannel,
    ) -> Result<Builder<NeedsConfiguring>> {
        let ftdi_serial = format!("{}{}", usb_serial, channel.as_char());
        self.with_ftdi_serial(&ftdi_serial)
    }

    /// Opens a device with the given description.
    pub fn with_description(self, desc: &str) -> Result<Builder<NeedsConfiguring>> {
        detach_all_ftdi_kernel_drivers();

        let ftdi_device = {
            let mut devices = FtdiGpio::list_devices()
                .wrap_err("failed to enumerate ftdi devices")?
                .filter(|di| di.description == desc);
            let Some(ftdi_device) = devices.next() else {
                bail!(
                    "failed to get any ftdi devices that match the description \"{desc}\""
                );
            };
            if devices.next().is_some() {
                bail!("multiple ftdi devices matched the description \"{desc}\"");
            }
            ftdi_device
        };

        let usb_device_info = {
            let mut devices = nusb::list_devices()
                .wait()
                .wrap_err("failed to enumerate devices")?
                .filter(|d| d.vendor_id() == ftdi_device.vendor_id)
                .filter(|d| d.product_id() == ftdi_device.product_id)
                .filter(|d| {
                    // See module-level docs for more info about missing serial numbers.
                    let sn = d.serial_number().unwrap_or("");
                    sn == "000000000"
                        || sn == strip_channel_suffix(&ftdi_device.serial_number)
                });

            let usb_device = devices.next().ok_or_eyre(
                "failed to find matching device in usbfs even though we found a \
                matching device from the FTDI library \
                , maybe the device was removed just after the check",
            )?;
            if devices.next().is_some() {
                bail!("multiple usb devices matched {ftdi_device:?}");
            }
            usb_device
        };

        let usb_device = usb_device_info
            .open()
            .wait()
            .wrap_err("failed to open usb device")?;
        for iinfo in usb_device_info.interfaces() {
            // Detaching the iface from other kernel drivers is necessary for
            // libftd2xx to work.
            // See also https://stackoverflow.com/a/34021765
            let _ = usb_device.detach_kernel_driver(iinfo.interface_number());
        }
        let ftdi = libftd2xx::Ftdi::with_description(desc).wrap_err_with(|| {
            format!("failed to open FTDI device with description \"{desc}\"")
        })?;

        Ok(Builder(NeedsConfiguring { device: ftdi }))
    }
}

impl Builder<NeedsConfiguring> {
    /// Configures the device into Async GPIO Bitbang mode.
    pub fn configure(mut self) -> Result<FtdiGpio> {
        const ALL_PINS_OUTPUT: u8 = 0xFF;
        self.0
            .device
            .set_bit_mode(ALL_PINS_OUTPUT, libftd2xx::BitMode::AsyncBitbang)
            .wrap_err("Failed to set device into async bitbang mode")?;
        let current_pin_state = read_pins(&mut self.0.device)
            .wrap_err("failed to read initial pin state")?;
        let device_info = self
            .0
            .device
            .device_info()
            .wrap_err("failed to get device info")?;
        Ok(FtdiGpio {
            device: self.0.device,
            desired_state: current_pin_state,
            device_info,
            is_destroyed: false,
        })
    }
}

/// An FTDI device configured in Async GPIO bitbang mode.
///
/// You can use this for controlling the pins of the FTDI adapter like a GPIO device.
pub struct FtdiGpio {
    device: libftd2xx::Ftdi,
    desired_state: u8,
    device_info: libftd2xx::DeviceInfo,
    is_destroyed: bool,
}

impl FtdiGpio {
    pub const CTS_PIN: Pin = Pin(3);
    pub const DTR_PIN: Pin = Pin(4);

    pub fn list_devices() -> Result<impl Iterator<Item = libftd2xx::DeviceInfo>> {
        libftd2xx::list_devices()
            .wrap_err("failed to list devices")
            .map(|d| d.into_iter())
    }

    /// Call this to construct an [`FtdiGpio`] using the builder pattern.
    ///
    /// # Example
    /// ```
    /// let ftdi = FtdiGpio::builder()
    ///     .with_default_device()?
    ///     .configure()?
    ///     
    /// ```
    pub fn builder() -> Builder<NeedsDevice> {
        Builder(NeedsDevice)
    }

    /// Controls the [`OutputState`] of the pin.
    pub fn set_pin(&mut self, pin: Pin, pin_state: OutputState) -> Result<()> {
        self.desired_state = compute_new_state(self.desired_state, pin, pin_state);
        write_pins(&mut self.device, self.desired_state)
    }

    /// Destroys the ftdi device, and fully resets its usb interface. Using this
    /// instead of Drop allows for explicit handling of errors.
    pub fn destroy(mut self) -> Result<()> {
        self.destroy_helper()
    }

    /// # Panics
    /// Panics if there is more than one matching device found. We don't have
    /// the ability to handle this case elegantly so better to just panic.
    fn destroy_helper(&mut self) -> Result<()> {
        if self.is_destroyed {
            return Ok(());
        }

        self.device.set_bit_mode(0, libftd2xx::BitMode::Reset)?;
        self.device.close()?;

        let devices: Vec<_> = nusb::list_devices()
            .wait()
            .wrap_err("failed to enumerate devices")?
            .filter(|d| d.vendor_id() == self.device_info.vendor_id)
            .filter(|d| d.product_id() == self.device_info.product_id)
            .filter(|d| {
                // See module-level docs for more info about missing serial numbers.
                let usb_serial = d.serial_number().unwrap_or("");
                // FTDI serial = USB serial + channel letter (A/B/C/D)
                // Strip the channel letter from FTDI serial for comparison
                let ftdi_serial = &self.device_info.serial_number;
                let ftdi_serial_base = strip_channel_suffix(ftdi_serial);
                tracing::debug!(
                    "serial: usb={usb_serial:?}, ftdi={ftdi_serial:?}, \
                     ftdi_base={ftdi_serial_base:?}"
                );
                usb_serial == "000000000" || usb_serial == ftdi_serial_base
            })
            .collect();

        if devices.is_empty() {
            bail!("no matching devices found");
        }
        if devices.len() > 1 {
            panic!("more than one matching device found");
        }
        let usb_device_info = devices.into_iter().next_back().unwrap();

        let usb_device = usb_device_info
            .open()
            .wait()
            .wrap_err("failed to open usb device")?;
        for iface in usb_device_info.interfaces() {
            let iface_num = iface.interface_number();
            let _ = usb_device.attach_kernel_driver(iface_num);
        }

        self.is_destroyed = true;

        Ok(())
    }
}

fn write_pins(device: &mut libftd2xx::Ftdi, pin_state: u8) -> Result<()> {
    let nbytes = device
        .write(&[pin_state])
        .wrap_err("failed to write to ftdi")?;
    ensure!(
        nbytes == 1,
        "number of bytes written did not match expected value"
    );
    Ok(())
}
fn read_pins(device: &mut libftd2xx::Ftdi) -> Result<u8> {
    let mut out_buf = [0u8; 1];
    let nbytes = device
        .read(&mut out_buf)
        .wrap_err("failed to read from ftdi")?;
    ensure!(
        nbytes == out_buf.len(),
        "failed to read out expected number of bytes"
    );
    Ok(out_buf[0])
}

/// Strips the channel suffix (A, B, C, D) from an FTDI serial to get the USB serial.
///
/// FTDI serial = USB serial + channel letter (e.g., "FT7ABC12C" → "FT7ABC12")
fn strip_channel_suffix(ftdi_serial: &str) -> &str {
    if ftdi_serial.is_empty() {
        return ftdi_serial;
    }
    let last_char = ftdi_serial.chars().last().unwrap();
    if matches!(last_char, 'A' | 'B' | 'C' | 'D') {
        let char_len = last_char.len_utf8();
        &ftdi_serial[..ftdi_serial.len() - char_len]
    } else {
        ftdi_serial
    }
}

impl Drop for FtdiGpio {
    fn drop(&mut self) {
        if let Err(err) = self.destroy_helper() {
            error!("failed to destroy FtdiGpio device: {err}")
        }
    }
}

/// Helper function for setting pins, sans-io. Broken out into sans-io helper function
/// for the purpose of supporting testing.
#[inline]
fn compute_new_state(current_state: u8, pin: Pin, output_state: OutputState) -> u8 {
    // Same idea as https://eblot.github.io/pyftdi/gpio.html#modifying-gpio-pin-state.
    // Zero out bit corresponding to `pin`.
    let cleared = current_state & !(1 << pin.0);
    // Set the state.
    cleared | ((output_state as u8) << pin.0)
}

#[cfg(test)]
mod test {
    use super::*;
    pub const RTS_PIN: Pin = Pin(2);

    #[derive(Debug)]
    struct Example {
        pin: Pin,
        output: OutputState,
        original: u8,
        expected: u8,
    }
    #[test]
    fn test_compute_new_state() {
        let examples = [
            Example {
                pin: RTS_PIN,
                output: OutputState::Low,
                original: 0b10111111,
                expected: 0b10111011,
            },
            Example {
                pin: RTS_PIN,
                output: OutputState::Low,
                original: 0b10111011,
                expected: 0b10111011,
            },
            Example {
                pin: RTS_PIN,
                output: OutputState::High,
                original: 0b10111011,
                expected: 0b10111111,
            },
            Example {
                pin: RTS_PIN,
                output: OutputState::High,
                original: 0b10111111,
                expected: 0b10111111,
            },
        ];
        for e in examples {
            let computed_state = compute_new_state(e.original, e.pin, e.output);
            assert_eq!(e.expected, computed_state, "failed example: {e:?}");
        }
    }
}
