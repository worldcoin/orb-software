use eyre::WrapErr;
use orb_update_agent_core::ManifestComponent;
use orb_update_agent_dbus::{
    ComponentState, ComponentStatus, UpdateProgress, UpdateStatus,
};
use tracing::warn;
use zbus::blocking::{object_server::InterfaceRef, Connection, ConnectionBuilder};

/// Create a DBus connection and initialize the UpdateProgress service.
///
/// This function establishes a session DBus connection and registers the
/// `UpdateProgress` interface at the `/org/worldcoin/UpdateProgress1` object path.
pub fn create_dbus_connection() -> eyre::Result<Connection> {
    ConnectionBuilder::session()
        .wrap_err("failed to establish user session DBus connection")?
        .name("org.worldcoin.UpdateProgress1")
        .wrap_err("failed to register the service under well-known name")?
        .serve_at(
            "/org/worldcoin/UpdateProgress1",
            UpdateProgress(UpdateStatus::default()),
        )
        .wrap_err("failed to serve at object path")?
        .build()
        .wrap_err("failed to initialize the service on DBus")
}

pub fn signal_error(error: &str, conn: &Connection) {
    if let Ok(iface) = get_iface_ref(conn) {
        iface.get_mut().0.error = error.to_string();
        emit_signal(&iface);
    } else {
        warn!("failed to get interface reference");
    }
}

pub fn init_components(components: &[ManifestComponent], conn: &Connection) {
    if let Ok(iface) = get_iface_ref(conn) {
        iface.get_mut().0.components = components
            .iter()
            .map(|c| ComponentStatus {
                name: c.name.clone(),
                state: ComponentState::None,
            })
            .collect();
    } else {
        warn!("failed to get interface reference");
    }
}

pub fn update_component_state(name: &str, state: ComponentState, conn: &Connection) {
    if let Ok(iface) = get_iface_ref(conn) {
        if let Some(component) = iface
            .get_mut()
            .0
            .components
            .iter_mut()
            .find(|c| c.name == name)
        {
            component.state = state;
        }
        emit_signal(&iface);
    } else {
        warn!("failed to get interface reference");
    }
}

fn emit_signal(iface: &InterfaceRef<UpdateProgress<UpdateStatus>>) {
    if let Err(err) =
        async_io::block_on(iface.get_mut().status_changed(iface.signal_context()))
    {
        warn!("failed to emit signal: {}", err);
    }
}

fn get_iface_ref(
    conn: &Connection,
) -> Result<InterfaceRef<UpdateProgress<UpdateStatus>>, zbus::Error> {
    let object_server = conn.object_server();
    object_server
        .interface::<_, UpdateProgress<UpdateStatus>>("org.worldcoin.UpdateProgress1")
}
