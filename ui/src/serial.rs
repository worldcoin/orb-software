//! Serial interface.

use std::io::Write;

use eyre::Result;
use futures::{channel::mpsc, prelude::*};
use prost::Message;

use orb_uart::{BaudRate, Device};
use tokio::runtime;

const SERIAL_DEVICE_DEFAULT: &str = "/dev/ttyTHS0";

/// A stub serial device that implements Write as a no-op
pub struct StubSerial;

impl Write for StubSerial {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub struct Serial {}

/// Serial interface.
impl Serial {
    /// Spawns a new serial interface with dependency injection.
    pub fn spawn<W: Write + Send + 'static>(
        mut writer: W,
        mut input_rx: mpsc::Receiver<orb_messages::mcu_message::Message>,
    ) -> Result<()> {
        let name = "mcu-uart-tx";
        std::thread::Builder::new()
            .name(name.to_string())
            .spawn(move || {
                let rt = runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("failed to create a new tokio runtime");
                while let Some(message) = rt.block_on(input_rx.next()) {
                    Self::write_message(&mut writer, message)
                        .expect("failed to transmit a message to MCU via UART");
                }
            })
            .expect("failed to spawn thread");

        Ok(())
    }

    /// Creates a real serial device writer
    pub fn create_real_device(serial_device: Option<&str>) -> Result<Device> {
        // macOS does not support baud rate higher than 115200 natively,
        // while we want to be able to compile and run tests on macOS,
        // we use a higher baud rate when running on the Jetson
        #[cfg(target_os = "macos")]
        let baud_rate = BaudRate::B115200;
        #[cfg(target_os = "linux")]
        let baud_rate = BaudRate::B1000000;

        Ok(Device::open(
            serial_device.unwrap_or(SERIAL_DEVICE_DEFAULT),
            baud_rate,
        )?)
    }

    #[allow(clippy::cast_possible_truncation)]
    fn write_message(
        w: &mut impl Write,
        message: orb_messages::mcu_message::Message,
    ) -> Result<()> {
        let message = orb_messages::McuMessage {
            version: orb_messages::Version::Version0 as i32,
            message: Some(message),
        };
        // UART message: magic (2B) + size (2B) + payload (protobuf-encoded McuMessage)
        let mut bytes = vec![0x8E, 0xAD];
        let mut payload = message.encode_length_delimited_to_vec();
        let mut size = Vec::from((payload.len() as u16).to_le_bytes());
        bytes.append(&mut size);
        bytes.append(&mut payload);
        w.write_all(&bytes)?;

        Ok(())
    }
}
