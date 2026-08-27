#[cfg(feature = "dbus")]
mod dbus_status;
pub mod intf_impl;
#[cfg(feature = "dbus")]
pub mod proxies;
#[cfg(feature = "zenoh")]
mod zenoh_status;

#[cfg(feature = "dbus")]
use color_eyre::eyre::{eyre, Result};
#[cfg(feature = "dbus")]
use orb_backend_status_dbus::{constants, BackendStatus, BackendStatusT};

#[cfg(feature = "dbus")]
pub async fn setup_dbus(
    conn: &zbus::Connection,
    backend_status_impl: impl BackendStatusT,
) -> Result<()> {
    conn.request_name(constants::SERVICE_NAME)
        .await
        .map_err(|e| eyre!("failed to request name on dbus {e}"))?;

    conn.object_server()
        .at(
            constants::OBJECT_PATH,
            BackendStatus::from(backend_status_impl),
        )
        .await
        .map_err(|e| eyre!("failed to serve obj on dbus {e}"))?;

    Ok(())
}
