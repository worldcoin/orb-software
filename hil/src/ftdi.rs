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
pub(crate) struct Pin(u8);

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

/// Parameters for selecting an FTDI device.
///
/// Either `serial_num` or `desc` can be specified to select a specific device.
/// If both are `None`, the default device will be used.
#[derive(Debug, Clone, Default, Eq, PartialEq, clap::Args)]
pub struct FtdiParams {
    /// The serial number of the FTDI device to use
    #[arg(long, conflicts_with = "desc")]
    pub serial_num: Option<String>,
    /// The description of the FTDI device to use
    #[arg(long, conflicts_with = "serial_num")]
    pub desc: Option<String>,
}

/// Type-state builder pattern for creating a [`FtdiGpio`].
#[derive(Clone, Debug)]
pub struct Builder<S>(S);

impl Builder<NeedsDevice> {
    /// Opens the first ftdi device identified. This can change across subsequent calls,
    /// if you need a specific device use [`Self::with_serial_number`] instead.
    ///
    /// Returns an error if there is more than 1 FTDI device connected.
    pub fn with_default_device(self) -> Result<Builder<NeedsConfiguring>> {
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
        if usb_device_infos.len() != 1 || ftdi_device_count != 1 {
            bail!("more than one FTDI device found");
        }
        let usb_device_info = usb_device_infos.into_iter().next_back().unwrap();

        // See module-level docs for more info about missing serial numbers.
        let serial_num = usb_device_info.serial_number().unwrap_or("");
        if !serial_num.is_empty() && serial_num != "000000000" {
            return self.with_serial_number(serial_num);
        }

        warn!("EEPROM is either blank or missing and there is no serial number");
        let mut device =
            libftd2xx::Ftdi::new().wrap_err("failed to open default ftdi device")?;
        let device_info = device.device_info().wrap_err("failed to get device info")?;
        debug!("using device: {device_info:?}");

        Ok(Builder(NeedsConfiguring { device }))
    }

    /// Opens a device with the given serial number.
    pub fn with_serial_number(self, serial: &str) -> Result<Builder<NeedsConfiguring>> {
        ensure!(!serial.is_empty(), "serial numbers cannot be empty");
        ensure!(
            serial != "000000000",
            "serial numbers cannot be the special zero serial"
        );

        let mut last_err = None;
        let usb_device_info = nusb::list_devices()
            .wait()
            .wrap_err("failed to enumerate devices")?
            .find(|d| d.serial_number() == Some(serial))
            .ok_or_else(|| {
                eyre!("usb device with matching serial \"{serial}\" not found")
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
            match libftd2xx::Ftdi::with_serial_number(serial).wrap_err_with(|| {
                format!("failed to open FTDI device with serial number \"{serial}\"")
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
            Err(eyre!("faild to find any ftdi devices"))
        }
    }

    /// Opens a device with the given description.
    pub fn with_description(self, desc: &str) -> Result<Builder<NeedsConfiguring>> {
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
                    sn == "000000000" || sn == ftdi_device.serial_number
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

    /// Opens a device based on the provided [`FtdiId`].
    pub fn with_id(self, id: FtdiId) -> Result<Builder<NeedsConfiguring>> {
        match id {
            FtdiId::SerialNumber(serial) => self.with_serial_number(&serial),
            FtdiId::Description(desc) => self.with_description(&desc),
        }
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
    pub const RTS_PIN: Pin = Pin(2);
    pub const CTS_PIN: Pin = Pin(3);

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

    /// # Panics
    /// Panics if there is more than one matching device found. We don't have
    /// the ability to handle this case elegantly so better to just panic.
    fn destroy_helper(&mut self) -> Result<()> {
        if self.is_destroyed {
            return Ok(());
        }

        self.device
            .set_bit_mode(0, libftd2xx::BitMode::Reset)
            .unwrap();
        self.device.close().unwrap();
        let devices: Vec<_> = nusb::list_devices()
            .wait()
            .wrap_err("failed to enumerate devices")?
            .filter(|d| d.vendor_id() == self.device_info.vendor_id)
            .filter(|d| d.product_id() == self.device_info.product_id)
            .filter(|d| {
                // See module-level docs for more info about missing serial numbers.
                let sn = d.serial_number().unwrap_or("");
                sn == "000000000" || sn == self.device_info.serial_number
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

impl crate::pin_controller::PinController for FtdiGpio {
    fn press_power_button(
        &mut self,
        duration: Option<std::time::Duration>,
    ) -> Result<()> {
        // Press the button (LOW = pressed)
        self.set_pin(Self::CTS_PIN, OutputState::Low)?;

        if let Some(duration) = duration {
            // Hold for the specified duration
            std::thread::sleep(duration);
            // Release the button (HIGH = released)
            self.set_pin(Self::CTS_PIN, OutputState::High)?;
        }

        Ok(())
    }

    fn set_recovery(&mut self, enabled: bool) -> Result<()> {
        let state = if enabled {
            OutputState::Low // Recovery mode
        } else {
            OutputState::High // Normal boot
        };
        self.set_pin(Self::RTS_PIN, state)
    }

    fn reset(&mut self) -> Result<()> {
        // Reset the FTDI device to default mode
        self.device
            .set_bit_mode(0, libftd2xx::BitMode::Reset)
            .wrap_err("failed to reset bit mode")?;

        // Re-enter async bitbang mode
        const ALL_PINS_OUTPUT: u8 = 0xFF;
        self.device
            .set_bit_mode(ALL_PINS_OUTPUT, libftd2xx::BitMode::AsyncBitbang)
            .wrap_err("failed to re-enter async bitbang mode")?;

        // Set all pins to HIGH (released/default state)
        self.desired_state = 0xFF;
        write_pins(&mut self.device, self.desired_state)
            .wrap_err("failed to write default pin state")?;

        Ok(())
    }

    fn turn_off(&mut self) -> Result<()> {
        self.press_power_button(Some(std::time::Duration::from_secs(10)))
    }

    fn turn_on(&mut self) -> Result<()> {
        self.press_power_button(Some(std::time::Duration::from_secs(4)))
    }

    fn destroy(&mut self) -> Result<()> {
        self.destroy_helper()
    }
}

#[cfg(test)]
mod test {
    use super::*;

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
                pin: FtdiGpio::RTS_PIN,
                output: OutputState::Low,
                original: 0b10111111,
                expected: 0b10111011,
            },
            Example {
                pin: FtdiGpio::RTS_PIN,
                output: OutputState::Low,
                original: 0b10111011,
                expected: 0b10111011,
            },
            Example {
                pin: FtdiGpio::RTS_PIN,
                output: OutputState::High,
                original: 0b10111011,
                expected: 0b10111111,
            },
            Example {
                pin: FtdiGpio::RTS_PIN,
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
