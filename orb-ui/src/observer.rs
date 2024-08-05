use crate::engine::EventChannel;
use crate::{dbus, tokio_spawn};
use eyre::{bail, Context, Result};
use futures::FutureExt;
use serde::{Deserialize, Serialize};
use tracing::info;
use zbus::export::futures_util::StreamExt;

const IFACE_PATH: &str = "/org/worldcoin/OrbUiState1";

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OrbUiState {
    pub state: String,
    pub progress: f64,
}

pub async fn listen(send_ui: &dyn EventChannel) -> Result<()> {
    let conn = zbus::Connection::session()
        .await
        .wrap_err("failed to connect to zbus session")?;
    let msg_stream = zbus::MessageStream::from(conn.clone());
    let dbus_wait_disconnected_task_handle =
        tokio_spawn("dbus_wait_disconnect", async move {
            // Until the stream terminates, this will never complete.
            let _ = msg_stream.count().await;
            bail!("dbus connection terminated");
        });

    // serve dbus interface
    // on session bus
    let _iface_ref: zbus::InterfaceRef<dbus::InboundInterface> = {
        let conn = zbus::ConnectionBuilder::session()
            .wrap_err("failed to establish user session dbus connection")?
            .name("org.worldcoin.OrbUiState1")
            .wrap_err("failed to get name")?
            .serve_at(IFACE_PATH, dbus::InboundInterface::new(send_ui.clone_tx()))
            .wrap_err("failed to serve at")?
            .build()
            .await
            .wrap_err("failed to build")?;
        let obj_serv = conn.object_server();
        obj_serv
            .interface(IFACE_PATH)
            .await
            .expect("should be successful because we already registered")
    };
    info!("serving dbus interface at {IFACE_PATH}");

    let _: ((),) = tokio::try_join!(dbus_wait_disconnected_task_handle
        .map(|r| r.wrap_err("dbus_wait_disconnected task exited unexpectedly")?))?;
    Ok(())
}
