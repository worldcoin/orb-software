#![forbid(unsafe_code)]

pub mod pin_controller;
mod remote_cmd;
mod ssh_wrapper;

#[path = "commands/ota/verify.rs"]
pub mod verify;

#[path = "commands/ota/mcu_util.rs"]
pub mod mcu_util;

pub use remote_cmd::{
    RemoteConnectArgs, RemoteSession, RemoteTransport, DEFAULT_SSH_USERNAME,
    DEFAULT_TELEPORT_USERNAME,
};
pub use ssh_wrapper::{AuthMethod, CommandResult, SshConnectArgs, SshWrapper};
