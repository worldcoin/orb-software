use clap::Parser;
use serialport::SerialPort;
use std::time::Duration;

use eyre::Result;

#[derive(Parser)]
#[command(author, version, about)]
struct Cli {
    #[arg(
        short = 'm',
        long = "modem",
        default_value = "/dev/ttyUSB2",
        help = "Path to the EC25 modem device"
    )]
    modem: String,

    #[arg(short = 'd', long = "debug", help = "Enables additional debug output")]
    debug: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let mut port = serialport::new(&cli.modem, 115200)
        .timeout(Duration::from_millis(1000))
        .open()?;

    let serving_cell = send_at_command(&mut port, "AT+QENG=\"servingcell\"")?;
    println!("Serving Cell Info:\n{}", serving_cell);

    let neighbor_cells = send_at_command(&mut port, "AT+QENG=\"neighbourcell\"")?;
    println!("Neighbor Cells Info:\n{}", neighbor_cells);

    let signal_quality = send_at_command(&mut port, "AT+QCSQ")?;
    println!("Signal Quality:\n{}", signal_quality);

    Ok(())
}

fn send_at_command(port: &mut Box<dyn SerialPort>, command: &str) -> Result<String> {
    let cmd = format!("{}\r\n", command);
    port.write_all(cmd.as_bytes())?;

    let mut response = String::new();
    let mut buf = [0u8; 1024];

    loop {
        match port.read(&mut buf) {
            Ok(n) if n > 0 => {
                response.push_str(&String::from_utf8_lossy(&buf[..n]));
                if response.contains("OK") || response.contains("ERROR") {
                    break;
                }
            }
            _ => break,
        }
    }
    Ok(response)
}
