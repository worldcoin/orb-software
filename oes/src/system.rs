use serde::{Deserialize, Serialize};

/// Boot ID cached by backend-status for periodic OES snapshots.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BootIdEvent {
    pub boot_id: String,
}
