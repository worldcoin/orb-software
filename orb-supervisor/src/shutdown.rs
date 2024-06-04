use std::{cmp::Ordering, fmt::Display, str::FromStr};

use tracing::info;
use zbus_systemd::login1::{self};
use Kind::{DryHalt, DryPoweroff, DryReboot, Halt, Poweroff, Reboot};

/// `ScheduledShutdown` represents the logind shutdown tuple as an argument
/// for the `org.freedesktop.login1.Manager.ScheduleShutdown` dbus method.
///
/// `Option<ScheduleShutdown>` represents the return value of the
/// `org.freedesktop.login1.Manager.ScheduleShutdown` property.
///
/// The priority of a scheduled shutdown is determined first by the kind (and
/// its presence), followed by the soonest (lowest `when` value).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScheduledShutdown {
    pub kind: Kind,
    pub when: u64,
}

impl ScheduledShutdown {
    /// `try_from_dbus` attempts to convert from the tuple returned by
    /// `org.freedesktop.login1.Manager.ScheduledShutdown` into a
    /// `Option<ScheduledShutdown>` instance.
    ///
    /// `org.freedesktop.login1.Manager.ScheduledShutdown` returns an empty
    /// string for `kind` (and `0` for `when`) if there is no already scheduled
    /// shutdown. In this case, we return `Ok(None)`
    pub fn try_from_dbus(
        (kind, when): (String, u64),
    ) -> Result<Option<Self>, UnknownShutdownKind> {
        let kind = match kind.parse::<Kind>() {
            Ok(kind) => kind,
            Err(KindParseErr::EmptyStr) => return Ok(None),
            Err(KindParseErr::Unknown(err)) => return Err(err),
        };

        Ok(Some(Self { kind, when }))
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

#[derive(Clone, Debug, PartialEq, PartialOrd, Eq, Ord)]
pub enum Kind {
    Poweroff = 6,
    Reboot = 5,
    Halt = 4,
    DryPoweroff = 3,
    DryReboot = 2,
    DryHalt = 1,
}

#[derive(thiserror::Error, Debug, Eq, PartialEq)]
#[error("unknown shutdown kind `{0}`")]
pub struct UnknownShutdownKind(String);

#[derive(thiserror::Error, Debug, Eq, PartialEq)]
pub enum KindParseErr {
    #[error(transparent)]
    Unknown(#[from] UnknownShutdownKind),
    #[error("empty string")]
    EmptyStr,
}

impl FromStr for Kind {
    type Err = KindParseErr;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(match value {
            "" => return Err(KindParseErr::EmptyStr),
            "dry-poweroff" => Kind::DryPoweroff,
            "dry-reboot" => Kind::DryReboot,
            "dry-halt" => Kind::DryHalt,
            "poweroff" => Kind::Poweroff,
            "reboot" => Kind::Reboot,
            "halt" => Kind::Halt,
            unknown => return Err(UnknownShutdownKind(unknown.to_owned()).into()),
        })
    }
}

impl Kind {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Poweroff => "poweroff",
            Reboot => "reboot",
            Halt => "halt",
            DryPoweroff => "dry-poweroff",
            DryReboot => "dry-reboot",
            DryHalt => "dry-halt",
        }
    }
}

impl Display for Kind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// The Happy path return value of [`schedule_shutdown`]. Describes whether there
/// were any existing scheduled shutdowns.
#[derive(Debug, Eq, PartialEq)]
pub enum PreemptionInfo {
    /// Scheduled successfully, and there were no existing shutdowns to preempt.
    NoExistingShutdown,
    /// The shutdown that was requested was ignored in favor of an existing shutdown.
    KeptExistingShutdown(ScheduledShutdown),
    /// The shutdown that was requested preempted/replaced an existing shutdown.
    PreemptedExistingShutdown(ScheduledShutdown),
}

/// Schedules a shutdown using `proxy`. Will preempt a pre-existing shutdown
/// of lower priority.
#[allow(clippy::missing_panics_doc)]
pub async fn schedule_shutdown(
    proxy: login1::ManagerProxy<'static>,
    shutdown_req: ScheduledShutdown,
) -> zbus::Result<PreemptionInfo> {
    let already_scheduled: Option<ScheduledShutdown> = {
        info!("getting property `org.freedesktop.login1.Manager.ScheduledShutdown`");
        let raw_tuple = proxy.scheduled_shutdown().await?;
        ScheduledShutdown::try_from_dbus(raw_tuple)
            .expect("infallible, the result should always parse")
    };

    let result = if let Some(already_scheduled) = already_scheduled {
        if shutdown_req.lt(&already_scheduled) {
            return Ok(PreemptionInfo::KeptExistingShutdown(already_scheduled));
        }
        PreemptionInfo::PreemptedExistingShutdown(already_scheduled)
    } else {
        PreemptionInfo::NoExistingShutdown
    };

    info!(
        "calling `org.freedesktop.login1.Manager.ScheduleShutdown` to shutdown system"
    );
    proxy
        .schedule_shutdown(shutdown_req.kind.as_str().to_owned(), shutdown_req.when)
        .await?;

    Ok(result)
}
