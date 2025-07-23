pub mod intf_impl;

use color_eyre::eyre::{Result, WrapErr};
use orb_backend_status_dbus::{BackendStatus, BackendStatusT, constants};
use tracing::error;
use zbus::ConnectionBuilder;

pub async fn setup_dbus(
    backend_status_impl: impl BackendStatusT,
) -> Result<zbus::Connection> {
    let dbus_conn = ConnectionBuilder::session()
        .wrap_err("failed creating a new session dbus connection")?
        .name(constants::SERVICE_NAME)
        .wrap_err(
            "failed to register dbus connection name: `org.worldcoin.BackendStatus1``",
        )?
        .serve_at(
            constants::OBJECT_PATH,
            BackendStatus::from(backend_status_impl),
        )
        .wrap_err("failed to serve dbus interface at `/org/worldcoin/BackendStatus1`")?
        .build()
        .await;

    let dbus_conn = match dbus_conn {
        Ok(conn) => conn,
        Err(e) => {
            error!("failed to setup dbus connection: {e:?}");
            return Err(e.into());
        }
    };

    Ok(dbus_conn)
}
