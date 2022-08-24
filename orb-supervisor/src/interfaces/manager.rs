//! [`Manager`] defines the `org.worldcoin.OrbSupervisor1.Manager` Dbus interface.
//!
//! It currently only supports the `BackgroundDownloadsAllowed` property used by the update to
//! decide whether or not it can download updates.

use tokio::time::{
    Duration,
    Instant,
};
use tracing::{
    debug,
    instrument,
    warn,
};
use zbus::{
    dbus_interface,
    Connection,
    SignalContext,
};
use zbus_systemd::systemd1;

/// The duration of time since the last "start signup" event that has to have passed
/// before the update agent is permitted to start a download.
pub const DEFAULT_DURATION_TO_ALLOW_DOWNLOADS: Duration = Duration::from_secs(3600);

pub const BACKGROUND_DOWNLOADS_ALLOWED_PROPERTY_NAME: &str = "BackgroundDownloadsAllowed";
pub const INTERFACE_NAME: &str = "org.worldcoin.OrbSupervisor1.Manager";
pub const OBJECT_PATH: &str = "/org/worldcoin/OrbSupervisor1/Manager";

pub struct Manager {
    duration_to_allow_downloads: Duration,
    last_signup_event: Instant,
    system_connection: Option<Connection>,
}

impl Manager {
    /// Constructs a new `Manager` instance.
    #[allow(clippy::must_use_candidate)]
    pub fn new() -> Self {
        Self {
            duration_to_allow_downloads: DEFAULT_DURATION_TO_ALLOW_DOWNLOADS,
            last_signup_event: Instant::now(),
            system_connection: None,
        }
    }

    #[must_use]
    pub fn duration_to_allow_downloads(self, duration_to_allow_downloads: Duration) -> Self {
        Self {
            duration_to_allow_downloads,
            ..self
        }
    }

    #[allow(clippy::must_use_candidate)]
    pub fn are_downloads_allowed(&self) -> bool {
        self.last_signup_event.elapsed() >= self.duration_to_allow_downloads
    }

    fn reset_last_signup_event(&mut self) {
        self.last_signup_event = Instant::now();
    }

    pub fn set_system_connection(&mut self, conn: zbus::Connection) {
        self.system_connection.replace(conn);
    }

    /// Resets the internal timer tracking the last signup event to the current time and emits a
    /// `PropertyChanged` for the `BackgroundDownloadsAllowed` signal.
    ///
    /// # Errors
    ///
    /// The same as calling [`zbus::fdo::Properties::properties_changed`].
    pub async fn reset_last_signup_event_and_notify(
        &mut self,
        signal_context: &SignalContext<'_>,
    ) -> zbus::Result<()> {
        self.reset_last_signup_event();
        self.background_downloads_allowed_changed(signal_context)
            .await
    }
}

impl Default for Manager {
    fn default() -> Self {
        Self::new()
    }
}

#[dbus_interface(name = "org.worldcoin.OrbSupervisor1.Manager")]
impl Manager {
    #[dbus_interface(property, name = "BackgroundDownloadsAllowed")]
    #[instrument(
        name = "org.worldcoin.OrbSupervisor1.Manager.BackgroundDownloadsAllowed",
        skip_all
    )]
    async fn background_downloads_allowed(&self) -> bool {
        debug!(
            millis = self.last_signup_event.elapsed().as_millis(),
            "time since last signup event",
        );
        self.are_downloads_allowed()
    }

    #[dbus_interface(name = "RequestUpdatePermission")]
    #[instrument(
        name = "org.worldcoin.OrbSupervisor1.RequestUpdatePermission",
        skip_all
    )]
    async fn request_update_permission(&self) -> zbus::fdo::Result<bool> {
        debug!("RequestUpdatePermission was called");
        let mut update_permitted = false;
        if let Some(conn) = &self.system_connection {
            let systemd_proxy = systemd1::ManagerProxy::new(conn).await?;
            match systemd_proxy
                .stop_unit("worldcoin-core.service".to_string(), "replace".to_string())
                .await
            {
                Ok(unit_path) => {
                    debug!(
                        job_object = unit_path.as_str(),
                        "`org.freedesktop.systemd1.Manager.StopUnit` returned"
                    );
                    update_permitted = true;
                }
                Err(zbus::Error::FDO(e)) => {
                    warn!(err = %e, "encountered a D-Bus error when calling `org.freedesktop.systemd1.Manager.StopUnit`");
                    update_permitted = true;
                }
                Err(e) => {
                    tracing::error!(err = ?e);
                    return Err(zbus::fdo::Error::ZBus(e));
                }
            };
        }
        Ok(update_permitted)
    }
}

#[cfg(test)]
mod tests {
    use zbus::Interface;

    use super::{
        Manager,
        DEFAULT_DURATION_TO_ALLOW_DOWNLOADS,
    };

    #[test]
    fn manager_interface_name_matches_exported_const() {
        assert_eq!(super::INTERFACE_NAME, &*Manager::name());
    }

    #[tokio::test]
    async fn manager_background_downloads_allowed_property_matched_exported_const() {
        let manager = Manager::new();
        assert!(
            manager
                .get(super::BACKGROUND_DOWNLOADS_ALLOWED_PROPERTY_NAME)
                .await
                .is_some()
        );
    }

    #[tokio::test(start_paused = true)]
    async fn downloads_are_disallowed_if_last_signup_event_is_too_recent() {
        let manager =
            Manager::new().duration_to_allow_downloads(DEFAULT_DURATION_TO_ALLOW_DOWNLOADS);
        tokio::time::advance(DEFAULT_DURATION_TO_ALLOW_DOWNLOADS / 2).await;

        assert!(!manager.are_downloads_allowed());
    }

    #[tokio::test(start_paused = true)]
    async fn downloads_are_allowed_if_last_signup_event_is_old_enough() {
        let manager =
            Manager::new().duration_to_allow_downloads(DEFAULT_DURATION_TO_ALLOW_DOWNLOADS);
        tokio::time::advance(DEFAULT_DURATION_TO_ALLOW_DOWNLOADS * 2).await;
        assert!(manager.are_downloads_allowed());
    }

    #[tokio::test(start_paused = true)]
    async fn downloads_become_disallowed_after_reset() {
        let mut manager =
            Manager::new().duration_to_allow_downloads(DEFAULT_DURATION_TO_ALLOW_DOWNLOADS);
        tokio::time::advance(DEFAULT_DURATION_TO_ALLOW_DOWNLOADS * 2).await;
        assert!(manager.are_downloads_allowed());
        manager.reset_last_signup_event();
        assert!(!manager.are_downloads_allowed());
    }
}
