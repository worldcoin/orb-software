use color_eyre::eyre::Result;
use orb_connd::{modem_manager::cli::ModemManagerCli, statsd::dd::DogstatsdClient};
use orb_info::orb_os_release::OrbOsRelease;
use tokio::signal::unix::{self, SignalKind};
use tracing::{info, warn};

const SYSLOG_IDENTIFIER: &str = "worldcoin-connd";

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let tel_flusher = orb_telemetry::TelemetryConfig::new()
        .with_journald(SYSLOG_IDENTIFIER)
        .init();

    let result = async {
        let tasks = orb_connd::program()
            .sysfs("/sys")
            .usr_persistent("/usr/persistent")
            .system_bus(zbus::Connection::system().await?)
            .session_bus(zbus::Connection::session().await?)
            .os_release(OrbOsRelease::read().await?)
            .statsd_client(DogstatsdClient::new())
            .modem_manager(ModemManagerCli)
            .run()
            .await?;

        let mut sigterm = unix::signal(SignalKind::terminate())?;
        let mut sigint = unix::signal(SignalKind::interrupt())?;

        tokio::select! {
            _ = sigterm.recv() => warn!("received SIGTERM"),
            _ = sigint.recv()  => warn!("received SIGINT"),
        }

        info!("aborting tasks and exiting gracefully");

        for handle in tasks {
            handle.abort();
        }

        Ok(())
    }
    .await;

    tel_flusher.flush().await;

    result
}
