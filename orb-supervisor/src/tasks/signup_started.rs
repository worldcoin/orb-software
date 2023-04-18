//! Listens for signup started signals from Orb Core.

use tokio::task::JoinHandle;
use tokio_stream::StreamExt as _;

use crate::{
    interfaces::Manager,
    proxies::core::SignupProxy,
    startup::Settings,
};

/// Spawns a task on the tokio runtime listening for `SignupStarted` D-Bus signals from Orb Core.
///
/// When the task receives a `SignupStarted` signal it resets the timer of the `Manager` interface,
/// and then sends out a `PropertiesChanged` signal for the `BackgroundDownloadsAllowed` property.
///
/// # Errors
///
/// + `[zbus::Error]` if an error occurred while building a D-Bus proxy listening for signups from
/// `orb-core`. The errors are the same as those in [`zbus::ProxyBuilder`].
pub async fn spawn_signup_started_task<'a>(
    settings: &Settings,
    connection: &'a zbus::Connection,
) -> zbus::Result<JoinHandle<zbus::Result<()>>> {
    let signup_proxy = SignupProxy::builder(connection)
        .destination(settings.signup_proxy_well_known_name.clone())?
        .path(settings.signup_proxy_object_path.clone())?
        .build()
        .await?;
    let mut signup_started = signup_proxy.receive_signup_started().await?;
    let conn = connection.clone();

    let manager_object_path = settings.manager_object_path.clone();
    let task_handle = tokio::spawn(async move {
        while signup_started.next().await.is_some() {
            let iface_ref = conn
                .object_server()
                .interface::<_, Manager>(manager_object_path.clone())
                .await?;
            let mut iface = iface_ref.get_mut().await;
            iface
                .reset_last_signup_event_and_notify(iface_ref.signal_context())
                .await?;
        }
        Ok::<_, zbus::Error>(())
    });
    Ok(task_handle)
}
