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

    /// Sends a command with retries and extended timeout
    fn send_command_with_retry(&mut self, command: &str, retries: usize, timeout_ms: u64) -> Result<String> {
        debug!("Sending AT command with retry: {}", command);
        let cmd = format!("{}\r\n", command);
        
        for attempt in 0..=retries {
            if attempt > 0 {
                debug!("Retry attempt {} for command: {}", attempt, command);
                // Wait before retrying
                std::thread::sleep(Duration::from_millis(500));
            }
            
            // Flush any pending data before sending
            self.port.flush()?;
            self.port.write_all(cmd.as_bytes())?;
            
            let mut response = String::new();
            let mut buf = [0u8; 1024];
            let start_time = std::time::Instant::now();
            
            while start_time.elapsed() < Duration::from_millis(timeout_ms) {
                match self.port.read(&mut buf) {
                    Ok(n) if n > 0 => {
                        response.push_str(&String::from_utf8_lossy(&buf[..n]));
                        if response.contains("OK") {
                            debug!("AT command response (success): {}", response);
                            return Ok(response);
                        } else if response.contains("ERROR") {
                            warn!("AT command returned error: {}", response);
                            break; // Error occurred, try next attempt
                        }
                    }
                    Ok(_) => {
                        // No data received, wait a bit
                        std::thread::sleep(Duration::from_millis(50));
                    }
                    Err(e) => {
                        warn!("Error reading from port: {}", e);
                        break; // Error occurred, try next attempt
                    }
                }
            }
            
            if response.contains("OK") {
                debug!("AT command response (after waiting): {}", response);
                return Ok(response);
            }
            
            warn!("Command timed out or failed: {}", command);
        }
        
        // If we got here, all retries failed
        Err(eyre::eyre!("Command failed after {} retries: {}", retries, command))
    }

    /// Issues AT+QENG="servingcell" and parses into a ServingCell.
    pub fn get_serving_cell(&mut self) -> Result<ServingCell> {
        let response = self.send_command_with_retry("AT+QENG=\"servingcell\"", 3, 5000)?;
        let cell = parse_serving_cell(&response)
            .wrap_err("Failed to parse serving cell info from the EC25 response")?;
        debug!(?cell, "Parsed serving cell info");
        Ok(cell)
    }

    /// Issues AT+QENG="neighbourcell" and parses into a list of NeighborCell.
    pub fn get_neighbor_cells(&mut self) -> Result<Vec<NeighborCell>> {
        let response = self.send_command_with_retry("AT+QENG=\"neighbourcell\"", 3, 5000)?;
        let cells = parse_neighbor_cells(&response).wrap_err_with(|| {
            "Failed to parse neighbor cell info from the EC25 response"
        })?;
        debug!(cell_count = cells.len(), "Parsed neighbor cells");
        Ok(cells)
    }

    /// Ensures the modem is ready by sending simple command and checking response
    pub fn ensure_modem_ready(&mut self) -> Result<()> {
        // Send AT command to check if modem is responsive
        let response = self.send_command_with_retry("AT", 5, 1000)?;
        if !response.contains("OK") {
            return Err(eyre::eyre!("Modem not responding properly"));
        }
        Ok(())
    }
    
    /// Reset the modem if it's in an inconsistent state
    pub fn reset_modem(&mut self) -> Result<()> {
        debug!("Resetting modem");
        let response = self.send_command_with_retry("AT+CFUN=1,1", 2, 10000)?;
        if !response.contains("OK") {
            return Err(eyre::eyre!("Failed to reset modem"));
        }
        // Wait for modem to complete reset
        std::thread::sleep(Duration::from_secs(5));
        Ok(())
    }
}
