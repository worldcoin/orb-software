use color_eyre::eyre::{self, OptionExt as _, Result, WrapErr as _};
use num_derive::{FromPrimitive, ToPrimitive};
use num_traits::FromPrimitive;
use orb_connd::{
    modem_manager::cli::ModemManagerCli, network_manager::NetworkManager,
    statsd::dd::DogstatsdClient, wpa_ctrl::cli::WpaCli, EntryPoint, ENV_FORK_MARKER,
};
use orb_info::orb_os_release::OrbOsRelease;
use std::{
    env::{self, VarError},
    str::FromStr,
    time::Duration,
};
use tokio::signal::unix::{self, SignalKind};
use tracing::{info, warn};

const SYSLOG_IDENTIFIER: &str = "worldcoin-connd";

fn main() -> Result<()> {
    color_eyre::install()?;
    match env::var(ENV_FORK_MARKER) {
        Ok(s) => {
            return EntryPoint::from_str(&s).expect("unknown entrypoint").run();
        }
        Err(VarError::NotUnicode(_)) => panic!("expected unicode env var value"),
        Err(VarError::NotPresent) => (),
    }

    tokio::runtime::Runtime::new()?.block_on(async_main())
}

async fn async_main() -> Result<()> {
    let tel_flusher = orb_telemetry::TelemetryConfig::new()
        .with_journald(SYSLOG_IDENTIFIER)
        .init();

    let result = async {
        let os_release = OrbOsRelease::read().await?;
        let nm = NetworkManager::new(
            zbus::Connection::system().await?,
            WpaCli::new(os_release.orb_os_platform_type),
        );

        let tasks = orb_connd::program()
            .sysfs("/sys")
            .usr_persistent("/usr/persistent")
            .network_manager(nm)
            .session_bus(zbus::Connection::session().await?)
            .os_release(os_release)
            .statsd_client(DogstatsdClient::new())
            .modem_manager(ModemManagerCli)
            .connect_timeout(Duration::from_secs(15))
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
