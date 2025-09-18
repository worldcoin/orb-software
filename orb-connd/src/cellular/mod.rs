use crate::{modem_manager, utils::run_cmd};
use color_eyre::{
    eyre::{bail, ContextCompat},
    Result,
};
use orb_info::orb_os_release::OrbRelease;
use rusty_network_manager::{
    ConnectionProxy, NetworkManagerProxy, SettingsConnectionProxy, SettingsProxy,
};
use std::time::Duration;
use tokio::{
    task::{self, JoinHandle},
    time,
};
use tracing::{error, info, warn};
use zbus::Connection;

const CONN_NAME: &str = "cellular";

pub async fn start(release: OrbRelease, dbus: &Connection) -> Result<()> {
    let nm_settings = SettingsProxy::new(dbus).await?;
    let conns = nm_settings.list_connections().await?;
    for c in conns {
        let c = SettingsConnectionProxy::new_from_path(c, dbus).await?;
        let s = c.get_settings().await?;
    }

    Ok(())
}
