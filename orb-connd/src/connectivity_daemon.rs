use crate::network_manager::NetworkManager;
use crate::resolved::Resolved;
use crate::service::{self, ConndService, ProfileStorage};
use crate::{ble, modem, reporters, OrbCapabilities};
use color_eyre::eyre::{Context, Result};
use orb_info::orb_os_release::OrbOsRelease;
use speare::mini::{self, OnErr};
use speare::{Backoff, Limit};
use std::path::Path;
use std::time::Duration;
use tracing::{error, info};
use zenorb::zenoh::bytes::Encoding;
use zenorb::Zenorb;

#[bon::builder(finish_fn = run)]
pub async fn program(
    sysfs: impl AsRef<Path>,
    procfs: impl AsRef<Path>,
    usr_persistent: impl AsRef<Path>,
    network_manager: NetworkManager,
    resolved: Resolved,
    session_bus: zbus::Connection,
    os_release: OrbOsRelease,
    connect_timeout: Duration,
    profile_storage: ProfileStorage,
    zenoh: &Zenorb,
) -> Result<mini::Ctx<()>> {
    let sysfs = sysfs.as_ref().to_path_buf();
    let procfs = procfs.as_ref().to_path_buf();

    let cap = OrbCapabilities::from_sysfs(&sysfs).await;

    info!(
        "connd starting on Orb {} {} with capabilities: {}",
        os_release.orb_os_platform_type, os_release.release_type, cap
    );

    let zsender = zenoh
        .sender()
        .publisher_with("oes/active_connections", |p| {
            p.encoding(Encoding::APPLICATION_JSON)
        })
        .publisher_with("oes/cellular_status", |p| {
            p.encoding(Encoding::APPLICATION_JSON)
        })
        .publisher_with("oes/netstats", |p| p.encoding(Encoding::APPLICATION_JSON))
        .build()
        .await?;

    let speare = speare::mini::root();

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

    let _ = zenoh
        .receiver(connd.clone())
        .queryable("job/wifi_add", service::zoci::wifi_add)
        .queryable("job/wifi_connect", service::zoci::wifi_connect)
        .queryable("job/wifi_remove", service::zoci::wifi_remove)
        .queryable("job/wifi_scan", service::zoci::wifi_scan)
        .queryable("job/wifi_list", service::zoci::wifi_list)
        .run()
        .await
        .inspect_err(|e| error!("failed to start connd zoci zenoh receiver: {e}"));

    let _ = speare
        .task_with()
        .on_err(OnErr::Restart {
            max: Limit::None,
            backoff: Backoff::Static(Duration::from_secs(30)),
        })
        .args(ble::Args {
            zenoh: zenoh.clone(),
        })
        .spawn(ble::advertiser)
        .inspect_err(|e| error!("failed to spawn ble beacon task: {e:?}"))?;

    speare.oneshot(async move |_| connd.spawn().await)?;

    reporters::spawn(
        &speare,
        network_manager,
        resolved,
        session_bus,
        zsender,
        sysfs,
        procfs,
    )
    .await?;

    if let OrbCapabilities::CellularAndWifi = cap {
        let _ = speare
            .task_with()
            .on_err(OnErr::Restart {
                max: 10.into(),
                backoff: Backoff::Incremental {
                    min: Duration::from_secs(10),
                    max: Duration::from_secs(100),
                    step: Duration::from_secs(10),
                },
            })
            .spawn(modem::supervisor)
            .inspect_err(|e| error!("failed to spawn modem supervisor: {e:?}"))?;
    }

    info!("finished connd startup");

    Ok(speare)
}
