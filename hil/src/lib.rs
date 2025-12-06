#![forbid(unsafe_code)]

mod ssh_wrapper;

#[path = "commands/ota/verify.rs"]
pub mod verify;

pub use ssh_wrapper::{AuthMethod, CommandResult, SshConnectArgs, SshWrapper};
