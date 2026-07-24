use chrono::{serde::ts_milliseconds, DateTime, Utc};
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
