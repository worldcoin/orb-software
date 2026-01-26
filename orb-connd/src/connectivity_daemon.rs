use crate::modem_manager::ModemManager;
use crate::network_manager::NetworkManager;
use crate::service::{ConndService, ProfileStorage};
use crate::statsd::StatsdClient;
use crate::{reporters, OrbCapabilities, Tasks};
use color_eyre::eyre::{OptionExt, Result};
use orb_info::orb_os_release::OrbOsRelease;
use std::time::Duration;
use std::{path::Path, sync::Arc};
use tokio::{task, time};
use tracing::info;
use tracing::{error, warn};
use zenorb::Zenorb;

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
    profile_storage: ProfileStorage,
    zenoh: &Zenorb,
) -> Result<Tasks> {
    let sysfs = sysfs.as_ref().to_path_buf();
    let modem_manager: Arc<dyn ModemManager> = Arc::new(modem_manager);

    let cap = OrbCapabilities::from_sysfs(&sysfs).await;

    info!(
        "connd starting on Orb {} {} with capabilities: {}",
        os_release.orb_os_platform_type, os_release.release_type, cap
    );

    let zsender = zenoh.sender().publisher("net/changed").build().await?;

    let connd = ConndService::new(
        session_bus.clone(),
        network_manager.clone(),
        os_release.release_type,
        cap,
        connect_timeout,
        &usr_persistent,
        profile_storage,
    )
    .await?;

    let mut tasks = vec![connd.spawn()];

    if let OrbCapabilities::CellularAndWifi = cap {
        setup_modem_bands_and_modes(&modem_manager);
    }

    tasks.extend(
        reporters::spawn(
            network_manager,
            session_bus,
            modem_manager,
            statsd_client,
            sysfs,
            cap,
            zsender,
        )
        .await?,
    );

    Ok(tasks)
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
            info!("modem bands set up successfully");

            match mm
                .set_allowed_and_preferred_modes(&modem.id, &["3g", "4g"], "4g")
                .await
            {
                Err(e) => warn!("allowed and preferred could not be set up: {e}"),
                Ok(_) => info!("allowed and preferred modes set up successfully"),
            };

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
