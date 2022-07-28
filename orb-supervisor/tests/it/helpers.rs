use std::io;

use dbus_launch::{
    BusType,
    Daemon,
};
use once_cell::sync::Lazy;
use orb_supervisor::{
    startup::{
        Application,
        Settings,
    },
    telemetry,
};
use tokio::{
    task::JoinHandle,
    time::Duration,
};
use tracing_subscriber::filter::LevelFilter;
use zbus::{
    dbus_interface,
    dbus_proxy,
    fdo,
    zvariant::OwnedObjectPath,
    ProxyDefault,
    SignalContext,
};

static TRACING: Lazy<()> = Lazy::new(|| {
    let filter = LevelFilter::DEBUG;
    if std::env::var("TEST_LOG").is_ok() {
        telemetry::start(filter, std::io::stdout);
    } else {
        telemetry::start(filter, std::io::sink);
    }
});

pub struct DbusInstances {
    pub session: Daemon,
    pub system: Daemon,
}

pub fn launch_dbuses() -> JoinHandle<io::Result<DbusInstances>> {
    tokio::task::spawn_blocking(|| {
        let session = launch_session_dbus()?;
        let system = launch_system_dbus()?;
        Ok(DbusInstances {
            session,
            system,
        })
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

pub async fn spawn_supervisor_service(settings: Settings) -> eyre::Result<Application> {
    Lazy::force(&TRACING);
    let application = Application::build(settings.clone()).await?;
    Ok(application)
}

#[dbus_proxy(interface = "org.worldcoin.OrbSupervisor1.Manager")]
pub trait Signup {
    #[dbus_proxy(property)]
    fn background_downloads_allowed(&self) -> zbus::Result<bool>;

    #[dbus_interface(name = "RequestUpdatePermission")]
    fn request_update_permission(&self) -> zbus::fdo::Result<bool>;
}

pub async fn make_update_agent_proxy<'a>(
    settings: &'a Settings,
    dbus_instances: &DbusInstances,
) -> zbus::Result<SignupProxy<'static>> {
    let connection = zbus::ConnectionBuilder::address(dbus_instances.session.address())?
        .build()
        .await?;
    SignupProxy::builder(&connection)
        .destination(settings.well_known_name.clone())?
        .path(settings.manager_object_path.clone())?
        .build()
        .await
}

struct Signup;

#[dbus_interface(name = "org.worldcoin.OrbCore1.Signup")]
impl Signup {
    #[dbus_interface(signal)]
    pub async fn signup_started(ctxt: &SignalContext<'_>) -> zbus::Result<()>;
}

pub async fn spawn_signup_start_task(
    settings: &Settings,
    dbus_instances: &DbusInstances,
) -> zbus::Result<JoinHandle<zbus::Result<()>>> {
    let conn = zbus::ConnectionBuilder::address(dbus_instances.session.address())?
        .name(settings.signup_proxy_well_known_name.clone())?
        .serve_at(settings.signup_proxy_object_path.clone(), Signup)?
        .build()
        .await?;

    let signup_proxy_object_path = settings.signup_proxy_object_path.clone();
    Ok(tokio::spawn(async move {
        loop {
            Signup::signup_started(&zbus::SignalContext::new(
                &conn,
                signup_proxy_object_path.clone(),
            )?)
            .await?;
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }))
}

struct Manager {
    tx: Option<tokio::sync::oneshot::Sender<(String, String)>>,
}

#[dbus_interface(name = "org.freedesktop.systemd1.Manager")]
impl Manager {
    #[dbus_interface(name = "StopUnit")]
    async fn stop_unit(&mut self, name: String, mode: String) -> fdo::Result<OwnedObjectPath> {
        tracing::debug!("StopUnit called");
        let tx = self
            .tx
            .take()
            .expect("Method must not be called more than once");
        tx.send((name.clone(), mode))
            .expect("Oneshot receiver must exist");
        OwnedObjectPath::try_from(
            format!("/org/freedesktop/systemd1/unit/{name}")
                .replace('-', "_2d")
                .replace('.', "_2e"),
        )
        .map_err(move |_| fdo::Error::UnknownObject(name))
    }
}

pub async fn start_systemd_manager(
    dbus_instances: &DbusInstances,
) -> zbus::Result<(
    zbus::Connection,
    tokio::sync::oneshot::Receiver<(String, String)>,
)> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let conn = zbus::ConnectionBuilder::address(dbus_instances.system.address())?
        .name(zbus_systemd::systemd1::ManagerProxy::DESTINATION)?
        .serve_at(
            zbus_systemd::systemd1::ManagerProxy::PATH,
            Manager {
                tx: tx.into(),
            },
        )?
        .build()
        .await?;
    Ok((conn, rx))
}
