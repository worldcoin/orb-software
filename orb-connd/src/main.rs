use color_eyre::eyre::Result;
use orb_connd::{modem_manager::cli::ModemManagerCli, statsd::dd::DogstatsdClient};
use orb_info::orb_os_release::OrbOsRelease;

const SYSLOG_IDENTIFIER: &str = "worldcoin-connd";

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let tel_flusher = orb_telemetry::TelemetryConfig::new()
        .with_journald(SYSLOG_IDENTIFIER)
        .init();

    let result = orb_connd::program()
        .sysfs("/sys")
        .system_dbus(zbus::Connection::system().await?)
        .session_dbus(zbus::Connection::session().await?)
        .os_release(OrbOsRelease::read().await?)
        .statsd_client(DogstatsdClient::new())
        .modem_manager(ModemManagerCli)
        .run()
        .await;

    tel_flusher.flush().await;

    result
}

// cdc-wdm0
