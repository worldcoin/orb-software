use clap::{Parser, Subcommand};
use color_eyre::eyre::Result;
use orb_connd::{
    connectivity_daemon,
    modem_manager::cli::ModemManagerCli,
    network_manager::NetworkManager,
    secure_storage::{self, SecureStorage},
    statsd::dd::DogstatsdClient,
    wpa_ctrl::cli::WpaCli,
};
use orb_info::orb_os_release::OrbOsRelease;
use orb_secure_storage_ca::in_memory::InMemoryBackend;
use std::time::Duration;
use tokio::{
    io,
    signal::unix::{self, SignalKind},
};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

const SYSLOG_IDENTIFIER: &str = "worldcoin-connd";

#[derive(Parser, Debug)]
pub struct Args {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    #[command(name = "connd")]
    ConnectivityDaemon,
    #[command(name = "ssd")]
    SecureStorageDaemon {
        #[arg(long)]
        in_memory: Option<bool>,
    },
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let tel_flusher = orb_telemetry::TelemetryConfig::new()
        .with_journald(SYSLOG_IDENTIFIER)
        .init();

    let args = Args::parse();

    use Command::*;
    let result = match args.command {
        ConnectivityDaemon => connectivity_daemon(),
        SecureStorageDaemon { in_memory } => {
            secure_storage_daemon(in_memory.unwrap_or_default())
        }
    };

    tel_flusher.flush_blocking();

    result
}

fn connectivity_daemon() -> Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    rt.block_on(async {
        let os_release = OrbOsRelease::read().await?;
        let nm = NetworkManager::new(
            zbus::Connection::system().await?,
            WpaCli::new(os_release.orb_os_platform_type),
        );

        let cancel_token = CancellationToken::new();
        let secure_storage =
            SecureStorage::new(std::env::current_exe()?, false, cancel_token.clone());

        let tasks = connectivity_daemon::program()
            .sysfs("/sys")
            .usr_persistent("/usr/persistent")
            .network_manager(nm)
            .session_bus(zbus::Connection::session().await?)
            .os_release(os_release)
            .statsd_client(DogstatsdClient::new())
            .modem_manager(ModemManagerCli)
            .connect_timeout(Duration::from_secs(15))
            .secure_storage(secure_storage)
            .run()
            .await?;

        let mut sigterm = unix::signal(SignalKind::terminate())?;
        let mut sigint = unix::signal(SignalKind::interrupt())?;

        tokio::select! {
            _ = sigterm.recv() => warn!("received SIGTERM"),
            _ = sigint.recv()  => warn!("received SIGINT"),
        }

        info!("aborting tasks and exiting gracefully");

        cancel_token.cancel();
        for handle in tasks {
            handle.abort();
        }

        Ok(())
    })
}

fn secure_storage_daemon(in_memory: bool) -> Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread().build()?;

    if in_memory {
        let mut ctx = orb_secure_storage_ca::in_memory::InMemoryContext::default();

        rt.block_on(secure_storage::subprocess::entry::<InMemoryBackend>(
            io::join(io::stdin(), io::stdout()),
            &mut ctx,
        ))
    } else {
        todo!()
    }
}
