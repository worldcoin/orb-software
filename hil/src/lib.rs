#![forbid(unsafe_code)]

mod ssh_wrapper;

#[path = "commands/ota/verify.rs"]
pub mod verify;

#[path = "commands/ota/mcu_util.rs"]
pub mod mcu_util;

pub use ssh_wrapper::{AuthMethod, CommandResult, SshConnectArgs, SshWrapper};
