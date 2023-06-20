use std::{
    cmp::Ordering,
    fmt::Display,
};

use futures::TryFutureExt;
use tokio::task::JoinHandle;
use tracing::{
    info,
    instrument,
};
use zbus_systemd::login1::{
    self,
    ManagerProxy,
};
use Kind::{
    DryHalt,
    DryPoweroff,
    DryReboot,
    Halt,
    Poweroff,
    Reboot,
};

/// `ScheduledShutdown` represents the logind shutdown tuple both as an argument
/// and return value for the `org.freedesktop.login1.Manager.ScheduledShutdown`
/// dbus method.
///
/// The priority of a scheduled shutdown is determined first by the kind (and
/// its presence), followed by the soonest (lowest `when` value).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScheduledShutdown {
    pub kind: Option<Kind>,
    pub when: u64,
}

#[derive(Clone, Debug, PartialEq, PartialOrd, Eq, Ord)]
pub enum Kind {
    Poweroff = 6,
    Reboot = 5,
    Halt = 4,
    DryPoweroff = 3,
    DryReboot = 2,
    DryHalt = 1,
}

impl ScheduledShutdown {
    /// `try_from_dbus` attempts to convert from the tuple returned by
    /// `org.freedesktop.login1.Manager.ScheduleShutdown` into a `ScheduledShutdown` instance
    ///
    /// # Errors
    /// This function bubbles up any errors encountered while calling `TryFrom<String>` for `Kind`
    pub fn try_from_dbus((kind, when): (String, u64)) -> Result<Self, Error> {
        let kind = match Kind::try_from(kind.as_str()) {
            Ok(kind) => Some(kind),
            Err(Error::NothingScheduled) => None,
            Err(err) => return Err(err),
        };

        Ok(Self {
            kind,
            when,
        })
    }

    #[must_use]
    pub fn kind_as_str(&self) -> String {
        match &self.kind {
            None => String::new(),
            Some(kind) => kind.to_string(),
        }
    }
}

impl PartialOrd for ScheduledShutdown {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match self.kind.cmp(&other.kind) {
            // We want to prioritize smaller `when` values
            Ordering::Equal => Some(self.when.cmp(&other.when).reverse()),
            v => Some(v),
        }
    }
}

impl TryFrom<&str> for Kind {
    type Error = Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "" => Err(Error::NothingScheduled),
            "dry-poweroff" => Ok(Self::DryPoweroff),
            "dry-reboot" => Ok(Self::DryReboot),
            "dry-halt" => Ok(Self::DryHalt),
            "poweroff" => Ok(Self::Poweroff),
            "reboot" => Ok(Self::Reboot),
            "halt" => Ok(Self::Halt),
            unknown => Err(Error::unrecognized_type(unknown)),
        }
    }
}

impl Display for Kind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Poweroff => "poweroff",
                Reboot => "reboot",
                Halt => "halt",
                DryPoweroff => "dry-poweroff",
                DryReboot => "dry-reboot",
                DryHalt => "dry-halt",
            }
        )
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("logind has no shutdown scheduled")]
    NothingScheduled,
    #[error("unrecognized shutdown type `{0}`")]
    UnrecognizedType(String),
    #[error("failed communicating over dbus")]
    Dbus(#[from] zbus::Error),
    #[error("deferring for higher priority scheduled shutdown `{0:?}`")]
    Defer(ScheduledShutdown),
}

impl Error {
    fn unrecognized_type(kind: &str) -> Self {
        Self::UnrecognizedType(kind.to_string())
    }
}

#[must_use]
pub fn spawn_logind_schedule_shutdown_task(
    proxy: login1::ManagerProxy<'static>,
    shutdown_req: ScheduledShutdown,
) -> JoinHandle<Result<(), Error>> {
    tokio::spawn(async move {
        info!("getting property `org.freedesktop.login1.Manager.ScheduledShutdown`");
        let scheduled_shutdown = get_logind_scheduled_shutdown(proxy.clone()).await?;

        // PartialOrd guarantees us that a requested "Poweroff" in 1000us will
        // take priority over an already scheduled "Reboot" in 10us
        if shutdown_req.gt(&scheduled_shutdown) {
            info!("calling `org.freedesktop.login1.Manager.ScheduleShutdown` to shutdown system");
            proxy
                .schedule_shutdown(shutdown_req.kind_as_str(), shutdown_req.when)
                .map_err(Error::from)
                .await
        } else {
            Err(Error::Defer(scheduled_shutdown))
        }
    })
}

#[instrument(skip_all, err, ret(Debug))]
async fn get_logind_scheduled_shutdown(
    proxy: ManagerProxy<'static>,
) -> Result<ScheduledShutdown, Error> {
    proxy
        .scheduled_shutdown()
        .map_ok(ScheduledShutdown::try_from_dbus)
        .map_err(Error::from)
        .await?
}
