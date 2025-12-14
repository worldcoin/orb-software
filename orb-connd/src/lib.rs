use color_eyre::eyre::{self, OptionExt as _, Result, WrapErr as _};
use derive_more::Display;
use futures::{SinkExt, TryStreamExt};
use modem_manager::ModemManager;
use network_manager::NetworkManager;
use num_derive::{FromPrimitive, ToPrimitive};
use num_traits::FromPrimitive;
use orb_info::orb_os_release::OrbOsRelease;
use service::ConndService;
use statsd::StatsdClient;
use std::str::FromStr;
use std::time::Duration;
use std::{path::Path, sync::Arc};
use tokio::{
    fs::{self},
    task::JoinHandle,
};
use tokio::{task, time};
use tracing::error;
use tracing::{info, warn};

pub mod key_material;
pub mod modem_manager;
pub mod network_manager;
pub mod service;
pub mod statsd;
pub mod storage_subprocess;
pub mod telemetry;
pub mod wpa_ctrl;

mod profile_store;
mod utils;

pub const ENV_FORK_MARKER: &str = "ORB_CONND_FORK_MARKER";

// TODO: Instead of toplevel enum, use inventory crate to register entry points and an
// init() hook at entry point of program.
#[derive(Debug, FromPrimitive, ToPrimitive)]
#[repr(u8)]
pub enum EntryPoint {
    SecureStorage = 1,
}

impl EntryPoint {
    pub fn run(self) -> Result<()> {
        let rt = tokio::runtime::Builder::new_current_thread().build()?;
        rt.block_on(match self {
            EntryPoint::SecureStorage => crate::storage_subprocess::entry(
                tokio::io::join(tokio::io::stdin(), tokio::io::stdout()),
            ),
        })
    }
}

impl FromStr for EntryPoint {
    type Err = eyre::Report;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Self::from_u8(u8::from_str(s).wrap_err("not a u8")?).ok_or_eyre("unknown id")
    }
}

#[bon::builder(finish_fn = run)]
pub async fn program(
    sysfs: impl AsRef<Path>,
    usr_persistent: impl AsRef<Path>,
    network_manager: NetworkManager,
    session_bus: zbus::Connection,
    os_release: OrbOsRelease,
    statsd_client: impl StatsdClient,
    modem_manager: impl ModemManager,
    connect_timeout: Duration,
) -> Result<Tasks> {
    let sysfs = sysfs.as_ref().to_path_buf();
    let modem_manager: Arc<dyn ModemManager> = Arc::new(modem_manager);

    {
        use crate::storage_subprocess::messages::{Request, Response};
        let mut storage_proc = crate::storage_subprocess::spawn_from_parent();
        storage_proc
            .send(Request::Get {
                key: String::from("foobar"),
            })
            .await?;
        let response = storage_proc.try_next().await?.expect("expected response");
        info!("got response: {response:?}");
    }
    info!("dropped storage");

    let cap = OrbCapabilities::from_sysfs(&sysfs).await;

    info!(
        "connd starting on Orb {} {} with capabilities: {}",
        os_release.orb_os_platform_type, os_release.release_type, cap
    );

    let connd = ConndService::new(
        session_bus.clone(),
        network_manager.clone(),
        os_release.release_type,
        cap,
        connect_timeout,
    );

    connd.setup_default_profiles().await?;

    if let Err(e) = connd.import_wpa_conf(&usr_persistent).await {
        warn!("failed to import legacy wpa config {e}");
    }

    if let Err(e) = connd.ensure_networking_enabled().await {
        warn!("failed to ensure networking is enabled {e}");
    }

    if let Err(e) = connd.ensure_nm_state_below_max_size(usr_persistent).await {
        warn!("failed to ensure nm state below max size: {e}");
    }

    let mut tasks = vec![connd.spawn()];

    if let OrbCapabilities::CellularAndWifi = cap {
        setup_modem_bands_and_modes(&modem_manager);
    }

    tasks.extend(
        telemetry::spawn(
            network_manager,
            session_bus,
            modem_manager,
            statsd_client,
            sysfs,
            cap,
        )
        .await?,
    );

    Ok(tasks)
}

pub(crate) type Tasks = Vec<JoinHandle<Result<()>>>;

#[derive(Display, Debug, PartialEq, Copy, Clone)]
pub enum OrbCapabilities {
    CellularAndWifi,
    WifiOnly,
}

impl OrbCapabilities {
    pub async fn from_sysfs(sysfs: impl AsRef<Path>) -> Self {
        let sysfs = sysfs.as_ref().join("class").join("net").join("wwan0");
        if fs::metadata(&sysfs).await.is_ok() {
            OrbCapabilities::CellularAndWifi
        } else {
            OrbCapabilities::WifiOnly
        }
    }
}

fn setup_modem_bands_and_modes(mm: &Arc<dyn ModemManager>) {
    let mm = Arc::clone(mm);

    task::spawn(async move {
        info!("trying to setup modem bands, allowed and preferred modes");

        let run = async || -> Result<()> {
            let modem = mm
                .list_modems()
                .await?
                .into_iter()
                .next()
                .ok_or_eyre("couldn't find a modem")?;

            let bands = [
                "egsm",
                "dcs",
                "pcs",
                "g850",
                "utran-1",
                "utran-2",
                "utran-4",
                "utran-5",
                "utran-6",
                "utran-8",
                "eutran-1",
                "eutran-2",
                "eutran-3",
                "eutran-4",
                "eutran-5",
                "eutran-7",
                "eutran-8",
                "eutran-9",
                "eutran-12",
                "eutran-13",
                "eutran-14",
                "eutran-18",
                "eutran-19",
                "eutran-20",
                "eutran-25",
                "eutran-26",
                "eutran-28",
            ];

            mm.set_current_bands(&modem.id, &bands).await?;
            mm.set_allowed_and_preferred_modes(&modem.id, &["3g", "4g"], "4g")
                .await?;

            info!("modem bands, allowed and preferred modes set up successfully");

            Ok(())
        };

        while let Err(e) = run().await {
            error!(
                    "failed to set up bands and preferred/allowed modes for modem: {e}. trying again in 10s"
                );

            time::sleep(Duration::from_secs(10)).await;
        }
    });
}
