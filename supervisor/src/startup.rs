use std::{path::PathBuf, time::Duration};

use color_eyre::eyre::WrapErr as _;
use futures::{future::TryFutureExt as _, FutureExt as _};
use tracing::debug;
use zbus::{Connection, ConnectionBuilder};
use zenorb::Zenorb;

use crate::{
    consts::DURATION_TO_STOP_CORE_AFTER_LAST_SIGNUP,
    interfaces::{self, manager, manager::DEFAULT_DURATION_TO_ALLOW_DOWNLOADS},
    proxies::core::{
        SIGNUP_PROXY_DEFAULT_OBJECT_PATH, SIGNUP_PROXY_DEFAULT_WELL_KNOWN_NAME,
    },
    tasks,
    tasks::zoci::GONDOR_BIN,
};

pub const DBUS_WELL_KNOWN_NAME: &str = "org.worldcoin.OrbSupervisor1";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed to establish connection to session dbus")]
    EstablishSessionConnection(#[source] zbus::Error),
    #[error("failed to establish connection to system dbus")]
    EstablishSystemConnection(#[source] zbus::Error),
    #[error("error occurred in zbus communication")]
    Zbus(#[from] zbus::Error),
    #[error("invalid session D-Bus address")]
    SessionDbusAddress(#[source] zbus::Error),
    #[error("error establishing a connection to the session D-Bus or registering an interface")]
    SessionDbusConnection,
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub session_dbus_path: Option<String>,
    pub system_dbus_path: Option<String>,
    pub manager_object_path: String,
    pub signup_proxy_well_known_name: String,
    pub signup_proxy_object_path: String,
    pub well_known_name: String,
    pub download_throttle: Duration,
    pub stop_core_after_signup: Duration,
    pub gondor_bin: PathBuf,
}

impl Settings {
    fn new() -> Self {
        Self {
            session_dbus_path: None,
            system_dbus_path: None,
            manager_object_path: manager::OBJECT_PATH.to_string(),
            signup_proxy_well_known_name: SIGNUP_PROXY_DEFAULT_WELL_KNOWN_NAME
                .to_string(),
            signup_proxy_object_path: SIGNUP_PROXY_DEFAULT_OBJECT_PATH.to_string(),
            well_known_name: DBUS_WELL_KNOWN_NAME.to_string(),
            download_throttle: DEFAULT_DURATION_TO_ALLOW_DOWNLOADS,
            stop_core_after_signup: DURATION_TO_STOP_CORE_AFTER_LAST_SIGNUP,
            gondor_bin: PathBuf::from(GONDOR_BIN),
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Application {
    pub session_connection: Connection,
    pub system_connection: Connection,
    pub settings: Settings,
    pub zenorb: Zenorb,
}

impl Application {
    /// Constructs an [`Application`] from [`Settings`].
    ///
    /// This function also connects to the session D-Bus instance.
    ///
    /// # Errors
    ///
    /// [`Application::build`] will return the following errors:
    ///
    /// * [`Error::SessionDbusAddress`], if the path to the socket holding the session D-Bus
    ///   instance was not understood (the path is conventionally stored in the environment
    ///   variable `$DBUS_SESSION_BUS_ADDRESS`, e.g. `unix:path=/run/user/1000/bus` and usually set
    ///   by systemd.
    /// * [`Error::EstablishSessionConnection`], if an error occurred while trying to establish
    ///   a connection to the session D-Bus instance, or trying to register an interface with it.
    ///   path to which is conventionally stored in the environment variable systemd.
    pub async fn build(
        settings: Settings,
        zenorb: Zenorb,
    ) -> Result<Application, Error> {
        let system_builder = if let Some(path) = settings.system_dbus_path.as_deref() {
            ConnectionBuilder::address(path)?
        } else {
            ConnectionBuilder::system()?
        };
        let system_connection = system_builder
            .name(settings.well_known_name.clone())?
            .build()
            .await
            .map_err(Error::EstablishSystemConnection)?;

        debug!(
            unique_bus_name = ?system_connection.unique_name(),
            "system dbus assigned unique bus name",
        );

        let mut manager = interfaces::Manager::new()
            .duration_to_allow_downloads(settings.download_throttle)
            .stop_core_after_signup(settings.stop_core_after_signup);
        manager.set_system_connection(system_connection.clone());

        let session_builder = if let Some(path) = settings.session_dbus_path.as_deref()
        {
            ConnectionBuilder::address(path)
        } else {
            ConnectionBuilder::session()
        }
        .map_err(Error::SessionDbusAddress)?;

        let session_connection = futures::future::ready(
            session_builder
                .name(settings.well_known_name.clone())
                .and_then(|builder| {
                    builder.serve_at(settings.manager_object_path.clone(), manager)
                }),
        )
        .and_then(ConnectionBuilder::build)
        .await
        .map_err(Error::EstablishSessionConnection)?;

        debug!(
            unique_bus_name = ?session_connection.unique_name(),
            "session dbus assigned unique bus name",
        );

        Ok(Self {
            session_connection,
            system_connection,
            settings,
            zenorb,
        })
    }

    /// Runs `Application` by spawning its constituent tasks.
    pub async fn run(self) -> color_eyre::Result<()> {
        let signup_started_task =
            tasks::spawn_signup_started_task(&self.settings, &self.session_connection)
                .await?;

        let _ =
            tasks::spawn_zoci_receiver(&self.zenorb, self.settings.gondor_bin.clone())
                .await
                .wrap_err("failed to spawn zoci receiver")?;

        let ((),) = tokio::try_join!(
            // All tasks are joined here
            signup_started_task.map(|e| e
                .wrap_err("signup_started task aborted unexpectedly")?
                .wrap_err("signup_started task exited with error")),
        )?;

        Ok(())
    }
}
