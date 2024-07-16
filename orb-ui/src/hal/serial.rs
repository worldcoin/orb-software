//! Serial interface.

use std::io::Write;
use std::time::Duration;

use eyre::Result;
use futures::{channel::mpsc, prelude::*};
use prost::Message;

use orb_uart::{BaudRate, Device};
use tokio::runtime;

const SERIAL_DEVICE: &str = "/dev/ttyTHS0";
const DELAY_BETWEEN_UART_MESSAGES_US: u64 = 200;

pub struct Serial {}

/// Serial interface.
impl Serial {
    /// Spawns a new serial interface.
    pub fn spawn(
        mut input_rx: mpsc::Receiver<orb_messages::mcu_main::mcu_message::Message>,
    ) -> Result<()> {
        // macOS does not support baud rate higher than 115200 natively,
        // while we want to be able to compile and run tests on macOS,
        // we use a higher baud rate when running on the Jetson
        #[cfg(target_os = "macos")]
        let baud_rate = BaudRate::B115200;
        #[cfg(target_os = "linux")]
        let baud_rate = BaudRate::B1000000;

        let mut device = Device::open(SERIAL_DEVICE, baud_rate)?;
        let name = "mcu-uart-tx";
        std::thread::Builder::new()
            .name(name.to_string())
            .spawn(move || {
                let rt = runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("failed to create a new tokio runtime");
                while let Some(message) = rt.block_on(input_rx.next()) {
                    Self::write_message(&mut device, message)
                        .expect("failed to transmit a message to MCU via UART");
                    // mark a pause between messages to avoid flooding the MCU
                    // and ensure that messages are correctly received
                    std::thread::sleep(Duration::from_micros(
                        DELAY_BETWEEN_UART_MESSAGES_US,
                    ));
                }
            })
            .expect("failed to spawn thread");

        Ok(())
    }

    #[allow(clippy::cast_possible_truncation)]
    fn write_message(
        w: &mut impl Write,
        message: orb_messages::mcu_main::mcu_message::Message,
    ) -> Result<()> {
        let message = orb_messages::mcu_main::McuMessage {
            version: orb_messages::mcu_main::Version::Version0 as i32,
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
