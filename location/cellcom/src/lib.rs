pub mod data;
pub mod parser;
pub mod types;

pub use data::{NeighborCell, ServingCell};
use eyre::{Context, Result};
use parser::{parse_neighbor_cells, parse_serving_cell};
use serialport::SerialPort;
use std::io::{Read, Write};
use std::time::Duration;
use tracing::{debug, warn};

/// Represents a connection to the EC25 modem for issuing QENG commands.
pub struct EC25Modem {
    // TODO: maybe genercize this idk
    port: Box<dyn SerialPort>,
}

impl EC25Modem {
    /// Opens the specified serial device and returns a new EC25Modem.
    pub fn new(device: &str) -> Result<Self> {
        let port = serialport::new(device, 115_200)
            .timeout(Duration::from_secs(2))
            .open()
            .wrap_err_with(|| format!("Failed to open serial port '{}'", device))?;

        Ok(Self { port })
    }

    /// Sends an AT command, returning the raw response string until "OK" or "ERROR".
    fn send_command(&mut self, command: &str) -> Result<String> {
        debug!("Sending AT command: {}", command);
        let cmd = format!("{}\r\n", command);
        self.port.write_all(cmd.as_bytes())?;
        let mut response = String::new();
        let mut buf = [0u8; 1024];

        loop {
            match self.port.read(&mut buf) {
                Ok(n) if n > 0 => {
                    response.push_str(&String::from_utf8_lossy(&buf[..n]));
                    if response.contains("OK") || response.contains("ERROR") {
                        break;
                    }
                }
                Ok(_) | Err(_) => break,
            }
        }

        if response.contains("ERROR") {
            warn!("AT command returned error: {}", response);
        } else {
            debug!("AT command response: {}", response);
        }

        Ok(response)
    }

    /// Issues AT+QENG="servingcell" and parses into a ServingCell.
    pub fn get_serving_cell(&mut self) -> Result<ServingCell> {
        let response = self.send_command("AT+QENG=\"servingcell\"")?;
        let cell = parse_serving_cell(&response)
            .wrap_err("Failed to parse serving cell info from the EC25 response")?;
        debug!(?cell, "Parsed serving cell info");
        Ok(cell)
    }

    /// Issues AT+QENG="neighbourcell" and parses into a list of NeighborCell.
    pub fn get_neighbor_cells(&mut self) -> Result<Vec<NeighborCell>> {
        let response = self.send_command("AT+QENG=\"neighbourcell\"")?;
        let cells = parse_neighbor_cells(&response).wrap_err_with(|| {
            "Failed to parse neighbor cell info from the EC25 response"
        })?;
        debug!(cell_count = cells.len(), "Parsed neighbor cells");
        Ok(cells)
    }
}
