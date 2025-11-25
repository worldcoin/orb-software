use color_eyre::eyre::Result;
use orb_connd::{
    key_material::static_key::StaticKey, modem_manager::cli::ModemManagerCli,
    network_manager::NetworkManager, statsd::dd::DogstatsdClient,
    wpa_ctrl::cli::WpaCli,
};
use orb_info::orb_os_release::OrbOsRelease;
use std::time::Duration;
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
        let nm = NetworkManager::new(zbus::Connection::system().await?, WpaCli);

        let tasks = orb_connd::program()
            .sysfs("/sys")
            .usr_persistent("/usr/persistent")
            .network_manager(nm)
            .session_bus(zbus::Connection::session().await?)
            .os_release(OrbOsRelease::read().await?)
            .statsd_client(DogstatsdClient::new())
            .modem_manager(ModemManagerCli)
            .connect_timeout(Duration::from_secs(15))
            .key_material(StaticKey(b"test".into()))
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
