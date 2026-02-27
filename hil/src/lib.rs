#![forbid(unsafe_code)]

mod boot;
pub mod commands;
mod download_s3;
mod ftdi;
mod nfsboot;
mod orb;
mod relay;
mod remote_cmd;
mod rts;
mod serial;
mod ssh_wrapper;

#[path = "commands/ota/verify.rs"]
pub mod verify;

#[path = "commands/ota/mcu_util.rs"]
pub mod mcu_util;

pub use remote_cmd::{RemoteConnectArgs, RemoteSession, RemoteTransport};
pub use ssh_wrapper::AuthMethod;

fn current_dir() -> camino::Utf8PathBuf {
    std::env::current_dir().unwrap().try_into().unwrap()
}
