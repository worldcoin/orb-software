//! Code to control GPIO of FTDI serial adapter

use color_eyre::{
    eyre::{ensure, eyre},
    eyre::{OptionExt, WrapErr as _},
    Result,
};
use libftd2xx::FtdiCommon;

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
use tracing::error;

/// Type-state builder pattern for creating a [`FtdiGpio`].
#[derive(Clone, Debug)]
pub struct Builder<S>(S);

impl Builder<NeedsDevice> {
    /// Opens the first ftdi device identified. This can change across subsequent calls,
    /// if you need a specific device use [`Self::with_serial_number`] instead.
    pub fn with_default_device(self) -> Result<Builder<NeedsConfiguring>> {
        let mut last_err = None;
        let dinfo_vec: Vec<_> = nusb::list_devices()
            .wrap_err("failed to list usb devices")?
            .filter(|dinfo| dinfo.vendor_id() == libftd2xx::FTDI_VID)
            .collect();
        for dinfo in dinfo_vec {
            let Some(serial) = dinfo.serial_number() else {
                continue;
            };
            let cloned = self.clone();
            match Self::with_serial_number(cloned, serial) {
                Ok(ftdi) => return Ok(ftdi),
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

    /// Opens a device with the given serial number.
    pub fn with_serial_number(self, serial: &str) -> Result<Builder<NeedsConfiguring>> {
        let mut last_err = None;
        let usb_device_info = nusb::list_devices()
            .wrap_err("failed to enumerate devices")?
            .find(|d| d.serial_number() == Some(serial))
            .ok_or_else(|| {
                eyre!("usb device with matching serial {serial} not found")
            })?;
        let usb_device = usb_device_info
            .open()
            .wrap_err("failed to open usb device")?;
        for iinfo in usb_device_info.interfaces() {
            // Detaching the iface from other kernel drivers is necessary for
            // libftd2xx to work.
            // See also https://stackoverflow.com/a/34021765
            let _ = usb_device.detach_kernel_driver(iinfo.interface_number());
            match libftd2xx::Ftdi::with_serial_number(serial).wrap_err_with(|| {
                format!("failed to open FTDI device with serial number {serial}")
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
        let serial = self
            .0
            .device
            .device_info()
            .wrap_err("faild to get device serial")?
            .serial_number;
        Ok(FtdiGpio {
            device: self.0.device,
            desired_state: current_pin_state,
            serial,
            is_destroyed: false,
        })
    }
}

/// An FTDI device configured in Async GPIO bitbang mode.
///
/// You can use this for controlling the pings of the FTDI adapter like a GPIO device.
pub struct FtdiGpio {
    device: libftd2xx::Ftdi,
    desired_state: u8,
    serial: String,
    is_destroyed: bool,
}

impl FtdiGpio {
    pub const RTS_PIN: Pin = Pin(2);
    pub const CTS_PIN: Pin = Pin(3);

    #[allow(dead_code)]
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

    fn destroy_helper(&mut self) -> Result<()> {
        if self.is_destroyed {
            return Ok(());
        }
        self.is_destroyed = true;

        self.device
            .set_bit_mode(0, libftd2xx::BitMode::Reset)
            .unwrap();
        self.device.close().unwrap();
        let usb_device_info = nusb::list_devices()
            .wrap_err("failed to enumerate devices")?
            .find(|d| d.serial_number() == Some(&self.serial))
            .ok_or_eyre("device with matching serial not found")?;
        let usb_device = usb_device_info
            .open()
            .wrap_err("failed to open usb device")?;
        for iface in usb_device_info.interfaces() {
            let iface_num = iface.interface_number();
            let _ = usb_device.attach_kernel_driver(iface_num);
        }

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
