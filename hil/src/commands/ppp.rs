use clap::Parser;
use clap::Subcommand;
/// Manage PPP connection to the orb.
use color_eyre::eyre::WrapErr;
use color_eyre::Result;
use std::net::Ipv4Addr;
use std::path::Path;
use std::process::Stdio;
use tokio;
use tokio::io::AsyncBufReadExt;
use tokio::process::Child;
use tokio::process::Command;
use tracing::error;
use tracing::info;

#[derive(Debug, Subcommand, Clone)]
pub enum Arg {
    Up,
    Down,
}

#[derive(Debug, Parser)]
pub struct PPP {
    #[command(subcommand)]
    cmd: Arg,
}

impl PPP {
    pub async fn run(self) -> Result<()> {
        match self.cmd {
            Arg::Up => {
                let ppp = Inner::up(
                    Path::new("/dev/ttyUSB0"),
                    1152000,
                    "169.254.1.22".parse().unwrap(),
                    "169.254.21.123".parse().unwrap(),
                )
                .await?;
                ppp.detach(Path::new("/tmp/orb-ppp.pid")).await?;
                Ok(())
            }
            Arg::Down => {
                unimplemented!()
            }
        }
    }
}

struct Inner {
    p: Child,
}

impl Inner {
    pub async fn up(
        serial_device: &Path,
        baud_rate: u32,
        local: Ipv4Addr,
        remote: Ipv4Addr,
    ) -> Result<Self> {
        let mut child = Command::new("pppd")
            .arg("nodetach")
            .arg(serial_device)
            .arg(baud_rate.to_string())
            .arg(format!("{}:{}", local, remote))
            .arg("noauth")
            .arg("local")
            .arg("lock")
            .arg("nocdtrcts")
            .kill_on_drop(true)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .wrap_err("Failed to run pppd")?;

        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        tokio::spawn(async move {
            let mut lines = tokio::io::BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                info!("PPP: {}", line)
            }
            info!("PPP: stdout done");
        });
        tokio::spawn(async move {
            let mut lines = tokio::io::BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                error!("PPP: {}", line)
            }
            info!("PPP: stderr done");
        });
        Ok(Inner { p: child })
    }

    /// Save the pid of the running pppd process to a file.
    pub async fn detach(self, pidfile: &Path) -> std::io::Result<()> {
        let pid = self.p.id().unwrap();
        info!("PPP: pid is {:?}", pid);
        tokio::fs::write(pidfile, pid.to_string()).await?;
        Ok(())
    }

    pub async fn from_pidfile(_pidfile: &Path) -> std::io::Result<Self> {
        unimplemented!()
    }
}

// TODO how to do async drop?
// impl Drop for Inner {
//     fn drop(&mut self) {
//         self.p.kill();
//         self.p.wait().await?;
//     }
// }
