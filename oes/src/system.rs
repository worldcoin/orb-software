use orb_contract_schema::contract;
use serde::{Deserialize, Serialize};

/// Boot ID cached by backend-status for periodic OES snapshots.
#[contract]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BootIdEvent {
    pub boot_id: String,
}
