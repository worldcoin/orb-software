use std::io;

use dbus_launch::{BusType, Daemon};
use once_cell::sync::Lazy;
use orb_supervisor::startup::{Application, Settings};
use tokio::task::JoinHandle;
use zbus::{
    fdo, interface, proxy, zvariant::OwnedObjectPath, ProxyDefault, SignalContext,
};

pub const WORLDCOIN_CORE_SERVICE_OBJECT_PATH: &str =
    "/org/freedesktop/systemd1/unit/worldcoin_2dcore_2eservice";
static TRACING: Lazy<()> = Lazy::new(|| {
    orb_telemetry::TelemetryConfig::new().init();
});

#[derive(Debug)]
pub struct DbusInstances {
    pub session: Daemon,
    pub system: Daemon,
}

pub fn launch_dbuses() -> JoinHandle<io::Result<DbusInstances>> {
    tokio::task::spawn_blocking(|| {
        let session = launch_session_dbus()?;
        let system = launch_system_dbus()?;
        Ok(DbusInstances { session, system })
    })
}

pub fn launch_session_dbus() -> io::Result<Daemon> {
    dbus_launch::Launcher::daemon()
        .bus_type(BusType::Session)
        .launch()
}

pub fn launch_system_dbus() -> io::Result<Daemon> {
    dbus_launch::Launcher::daemon()
        .bus_type(BusType::System)
        .launch()
}

pub fn make_settings(dbus_instances: &DbusInstances) -> Settings {
    Settings {
        session_dbus_path: dbus_instances.session.address().to_string().into(),
        system_dbus_path: dbus_instances.system.address().to_string().into(),
        ..Default::default()
    }
}

pub async fn spawn_supervisor_service(
    settings: Settings,
) -> color_eyre::Result<Application> {
    Lazy::force(&TRACING);
    let application = Application::build(settings.clone()).await?;
    Ok(application)
}

#[proxy(
    interface = "org.worldcoin.OrbSupervisor1.Manager",
    gen_async = true,
    gen_blocking = false,
    default_service = "org.worldcoin.OrbSupervisor1",
    default_path = "/org/worldcoin/OrbSupervisor1/Manager"
)]
pub trait Signup {
    #[zbus(property)]
    fn background_downloads_allowed(&self) -> zbus::Result<bool>;

    #[zbus(name = "RequestUpdatePermission")]
    fn request_update_permission(&self) -> zbus::fdo::Result<()>;
}

pub async fn make_update_agent_proxy<'a>(
    settings: &'a Settings,
    dbus_instances: &DbusInstances,
) -> zbus::Result<SignupProxy<'static>> {
    let connection =
        zbus::ConnectionBuilder::address(dbus_instances.session.address())?
            .build()
            .await?;
    SignupProxy::builder(&connection)
        .cache_properties(zbus::CacheProperties::No)
        .destination(settings.well_known_name.clone())?
        .path(settings.manager_object_path.clone())?
        .build()
        .await
}

struct Signup;

#[interface(name = "org.worldcoin.OrbCore1.Signup")]
impl Signup {
    #[zbus(signal)]
    pub(crate) async fn signup_started(ctxt: &SignalContext<'_>) -> zbus::Result<()>;
}

pub async fn start_signup_service_and_send_signal(
    settings: &Settings,
    dbus_instances: &DbusInstances,
) -> zbus::Result<()> {
    let conn = zbus::ConnectionBuilder::address(dbus_instances.session.address())?
        .name(settings.signup_proxy_well_known_name.clone())?
        .serve_at(settings.signup_proxy_object_path.clone(), Signup)?
        .build()
        .await?;

    let signup_proxy_object_path = settings.signup_proxy_object_path.clone();
    Signup::signup_started(&zbus::SignalContext::new(
        &conn,
        signup_proxy_object_path.clone(),
    )?)
    .await?;
    Ok(())
}

struct Manager;

#[interface(name = "org.freedesktop.systemd1.Manager")]
impl Manager {
    #[zbus(name = "GetUnit")]
    async fn get_unit(&self, name: String) -> fdo::Result<OwnedObjectPath> {
        tracing::debug!(name, "GetUnit called");
        match &*name {
            "worldcoin-core.service" => {
                OwnedObjectPath::try_from(WORLDCOIN_CORE_SERVICE_OBJECT_PATH)
            }
            _other => OwnedObjectPath::try_from(
                format!("/org/freedesktop/systemd1/unit/{name}")
                    .replace('-', "_2d")
                    .replace('.', "_2e"),
            ),
        }
        .map_err(move |_| fdo::Error::UnknownObject(name))
    }

    #[zbus(name = "StopUnit")]
    async fn stop_unit(
        &self,
        name: String,
        _mode: String,
    ) -> fdo::Result<OwnedObjectPath> {
        tracing::debug!(name, _mode, "StopUnit called");
        OwnedObjectPath::try_from("/org/freedesktop/systemd1/job/1234")
            .map_err(move |_| fdo::Error::UnknownObject(name))
    }
}

pub struct CoreUnit {
    active_state: String,
}

#[interface(name = "org.freedesktop.systemd1.Unit")]
impl CoreUnit {
    #[zbus(property)]
    pub async fn active_state(&self) -> String {
        tracing::debug!("ActiveState property requested");
        self.active_state.clone()
    }

    #[zbus(property)]
    pub async fn set_active_state(&mut self, active_state: String) {
        tracing::debug!(active_state, "SetActiveState property called");
        self.active_state = active_state;
    }
}

pub struct CoreService;

#[interface(name = "org.freedesktop.systemd1.Service")]
impl CoreService {
    #[zbus(property, name = "TimeoutStopUSec")]
    async fn timeout_stop_u_sec(&self) -> u64 {
        tracing::debug!("TimeoutStopUSec property requested");
        20_000_000
    }
}

pub async fn start_interfaces(
    dbus_instances: &DbusInstances,
) -> zbus::Result<zbus::Connection> {
    let conn = zbus::ConnectionBuilder::address(dbus_instances.system.address())?
        .name(zbus_systemd::systemd1::ManagerProxy::DESTINATION.unwrap())?
        .serve_at(zbus_systemd::systemd1::ManagerProxy::PATH.unwrap(), Manager)?
        .serve_at(WORLDCOIN_CORE_SERVICE_OBJECT_PATH, CoreService)?
        .serve_at(
            WORLDCOIN_CORE_SERVICE_OBJECT_PATH,
            CoreUnit {
                active_state: "active".into(),
            },
        )?
        .build()
        .await?;
    Ok(conn)
}
