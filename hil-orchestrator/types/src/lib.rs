use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunnerId(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Diamond,
    Pearl,
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Platform::Diamond => write!(f, "diamond"),
            Platform::Pearl => write!(f, "pearl"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatRequest {
    pub hostname: String,
    pub platform: Platform,
    pub locked: bool,
    pub current_job: Option<String>,
    pub current_run_id: Option<String>,
    pub pr_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunnerStatus {
    pub id: String,
    pub hostname: String,
    pub platform: Platform,
    pub locked: bool,
    pub online: bool,
    pub current_job: Option<String>,
    pub current_run_id: Option<String>,
    #[serde(default)]
    pub pr_ref: Option<String>,
    pub last_heartbeat: i64, // Unix epoch seconds
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingCommand {
    pub id: i64,
    pub command: CommandKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CommandKind {
    Lock,
    Unlock,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostResultRequest {
    pub runner_id: String,
    pub rts_name: String,
    pub github_run_id: String,
    pub pr_number_or_commit: Option<String>,
    pub platform: Platform,
    pub result: TestOutcome,
    pub content: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResultRecord {
    pub id: i64,
    pub runner_id: String,
    pub rts_name: String,
    pub github_run_id: String,
    pub pr_ref: Option<String>,
    pub platform: Platform,
    pub result: TestOutcome,
    pub recorded_at: i64, // Unix epoch seconds
    pub content: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TestOutcome {
    Pass,
    Fail,
    Error,
}
