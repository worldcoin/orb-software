use std::io;

use dbus_launch::{
    BusType,
    Daemon,
};
use orb_supervisor::startup::{
    Application,
    Settings,
};
use tokio::{
    task::JoinHandle,
    time::Duration,
};
use zbus::{
    dbus_interface,
    dbus_proxy,
    SignalContext,
};

pub struct DbusInstances {
    pub session: Daemon,
}

pub fn launch_dbuses() -> JoinHandle<io::Result<DbusInstances>> {
    tokio::task::spawn_blocking(|| {
        let session = launch_session_dbus()?;
        Ok(DbusInstances {
            session,
        })
    })
}

pub fn launch_session_dbus() -> io::Result<Daemon> {
    dbus_launch::Launcher::daemon()
        .bus_type(BusType::Session)
        .launch()
}

pub fn make_settings(dbus_instances: &DbusInstances) -> Settings {
    Settings {
        session_dbus_path: dbus_instances.session.address().to_string().into(),
        ..Default::default()
    }
}

pub async fn spawn_supervisor_service(settings: Settings) -> eyre::Result<Application> {
    let application = Application::build(settings.clone()).await?;
    Ok(application)
}

#[dbus_proxy(interface = "org.worldcoin.OrbSupervisor1.Manager")]
pub trait Signup {
    #[dbus_proxy(property)]
    fn background_downloads_allowed(&self) -> zbus::Result<bool>;
}

pub async fn make_update_agent_proxy<'a>(
    settings: &'a Settings,
    dbus_instances: &DbusInstances,
) -> zbus::Result<SignupProxy<'a>> {
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
