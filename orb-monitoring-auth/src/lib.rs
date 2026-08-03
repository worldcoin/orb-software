//! Defines the monitoring token exchanged with the backend and served to the
//! Datadog authentication client.
//!
//! Token freshness decisions are pure: callers supply the time at which a token
//! is evaluated.

use chrono::{serde::ts_milliseconds, DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};

pub mod client;
pub mod server;

pub const DD_AGENT_UID: u32 = 107;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Token {
    token: String,
    #[serde(with = "ts_milliseconds")]
    expiry: DateTime<Utc>,
}

impl Token {
    /// Reports whether the token is unusable at `now`.
    ///
    /// A token expires at its recorded expiry instant, so equality counts as
    /// expired. This decision performs no I/O and does not read the system clock.
    pub(crate) fn is_expired_at(&self, now: DateTime<Utc>) -> bool {
        self.expiry <= now
    }

    /// Reports whether the token must be replaced at `now`.
    ///
    /// Monitoring tokens are refreshed when less than 180 days remain. This
    /// decision performs no I/O and does not read the system clock.
    pub(crate) fn needs_refresh_at(&self, now: DateTime<Utc>) -> bool {
        self.expiry - now < TimeDelta::days(180)
    }
}
