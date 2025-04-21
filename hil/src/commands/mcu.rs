use color_eyre::{
    eyre::{bail, eyre, WrapErr as _},
    Result, Section as _,
};
use tracing::info;

#[derive(Debug, clap::Parser)]
pub struct Mcu {
    #[clap(subcommand)]
    subcommands: Subcommands,
}

impl Mcu {
    pub async fn run(self) -> Result<()> {
        self.subcommands.run().await
    }
}

/// Microcontroller utilities
#[derive(Debug, clap::Parser)]
enum Subcommands {
    Rdp(RdpCommand),
}

impl Subcommands {
    async fn run(self) -> Result<()> {
        match self {
            Subcommands::Rdp(c) => c.run().await,
        }
    }
}

/// Control read-out protection.
///
/// Requires a hardware debugger/probe.
#[derive(Debug, clap::Parser)]
struct RdpCommand {
    /// The USB serial number of the probe to use
    #[clap(long)]
    serial: Option<String>,
    /// vendor_id:[product_id]
    #[clap(long, value_parser = usb_device_parser)]
    device: Option<(u16, u16)>,
}

fn usb_device_parser(s: &str) -> Result<(u16, u16)> {
    let (vid, pid) = match s.split_once(':') {
        Some((vid, pid)) => {
            let pid = u16::from_str_radix(pid, 16)
                .wrap_err("expected base16 for productid")?;
            (vid, pid)
        }
        None => (s, 0),
    };
    let vid = u16::from_str_radix(vid, 16).wrap_err("expected base16 for vendorid")?;
    Ok((vid, pid))
}

impl RdpCommand {
    async fn run(self) -> Result<()> {
        let lister = probe_rs::probe::list::Lister::new();
        let probes = lister.list_all();
        if probes.len() == 0 {
            return Err(eyre!("no debug probes found"))
                .suggestion(
                    "make sure a hardware probe/debugger is connected to your \
                    computer",
                )
                .suggestion(
                    "make sure your udev rules are configured and you can read from \
                    usb",
                );
        }
        info!("Found probes:");
        for p in probes.iter() {
            info!("{p}");
        }
        let Some(probe) = probes
            .into_iter()
            .filter(|p| {
                self.serial
                    .as_ref()
                    .map(|expected_serial| {
                        p.serial_number.as_deref().unwrap_or_default()
                            == expected_serial
                    })
                    .unwrap_or(true)
            })
            .filter(|p| {
                self.device
                    .map(|(expected_vid, expected_pid)| {
                        p.vendor_id == expected_vid
                            && (p.product_id == expected_pid || expected_pid == 0)
                    })
                    .unwrap_or(true)
            })
            .next()
        else {
            bail!("failed to filter probes based on command line arguments");
        };
        info!("using probe {probe}");

        todo!()
    }
}
