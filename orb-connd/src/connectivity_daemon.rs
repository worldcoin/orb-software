use crate::mcu_util::McuUtil;
use crate::modem_manager::ModemManager;
use crate::network_manager::NetworkManager;
use crate::resolved::Resolved;
use crate::service::{ConndService, ProfileStorage};
use crate::statsd::StatsdClient;
use crate::systemd::Systemd;
use crate::{modem, reporters, OrbCapabilities};
use color_eyre::eyre::{Context, Result};
use orb_info::orb_os_release::OrbOsRelease;
use speare::mini::{self, OnErr};
use speare::Backoff;
use std::time::Duration;
use std::{path::Path, sync::Arc};
use tracing::info;
use zenorb::zenoh::bytes::Encoding;
use zenorb::Zenorb;

#[bon::builder(finish_fn = run)]
pub async fn program(
    sysfs: impl AsRef<Path>,
    procfs: impl AsRef<Path>,
    usr_persistent: impl AsRef<Path>,
    network_manager: NetworkManager,
    systemd: Systemd,
    resolved: Resolved,
    session_bus: zbus::Connection,
    os_release: OrbOsRelease,
    statsd_client: impl StatsdClient,
    modem_manager: impl ModemManager,
    mcu_util: impl McuUtil,
    connect_timeout: Duration,
    profile_storage: ProfileStorage,
    zenoh: &Zenorb,
) -> Result<mini::Ctx<()>> {
    let sysfs = sysfs.as_ref().to_path_buf();
    let procfs = procfs.as_ref().to_path_buf();
    let modem_manager: Arc<dyn ModemManager> = Arc::new(modem_manager);
    let mcu_util: Arc<dyn McuUtil> = Arc::new(mcu_util);
    let statsd_client: Arc<dyn StatsdClient> = Arc::new(statsd_client);

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

    speare.oneshot(async move |_| connd.spawn().await)?;

    reporters::spawn(
        &speare,
        network_manager,
        resolved,
        session_bus,
        statsd_client,
        zsender,
        sysfs,
        procfs,
    )
    .await?;

    if let OrbCapabilities::CellularAndWifi = cap {
        speare
            .task_with()
            .args(modem::Args {
                poll_interval: Duration::from_secs(30),
                modem_manager,
                mcu_util,
                systemd,
            })
            .on_err(OnErr::Restart {
                max: 10.into(),
                backoff: Backoff::Incremental {
                    min: Duration::from_secs(10),
                    max: Duration::from_secs(100),
                    step: Duration::from_secs(10),
                },
            })
            .spawn(modem::supervisor)
            .wrap_err("failed to spawn modem supervisor")?;
    }

    info!("finished connd startup");

    Ok(speare)
}
