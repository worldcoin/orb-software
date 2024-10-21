use color_eyre::eyre::{eyre, Result, WrapErr as _};
use futures::StreamExt;
use zbus::fdo::DBusProxy;
use zbus::Connection;

pub mod core;

/// Returns after `name` appears on dbus.
pub async fn wait_for_dbus_registration(conn: &Connection, name: &str) -> Result<()> {
    let dbus = DBusProxy::new(conn)
        .await
        .wrap_err("failed to create org.freedesktop.DBus proxy object")?;
    let mut name_changed = dbus
        .receive_name_owner_changed()
        .await
        .wrap_err("failed to get NameOwnerChanged signal stream")?;
    while let Some(c) = name_changed.next().await {
        let a = c.args().wrap_err("failed to extract signal args")?;
        if a.name == name {
            return Ok(());
        }
    }
    Err(eyre!("NameOwnerChanged stream unexpectedly ended"))
}
