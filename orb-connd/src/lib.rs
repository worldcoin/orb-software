use color_eyre::{
    eyre::{self, Context, OptionExt},
    Result,
};
use derive_more::Display;
use num_derive::{FromPrimitive, ToPrimitive};
use num_traits::FromPrimitive as _;
use orb_secure_storage::in_memory::InMemoryBackend;
use std::env::VarError;
use std::path::Path;
use std::str::FromStr;
use tokio::{fs, task::JoinHandle};

pub mod key_material;
pub mod main_daemon;
pub mod modem_manager;
pub mod network_manager;
pub mod service;
pub mod statsd;
pub mod telemetry;
pub mod wpa_ctrl;

mod profile_store;
mod secure_storage;
mod utils;

pub(crate) type Tasks = Vec<JoinHandle<Result<()>>>;

#[derive(Display, Debug, PartialEq, Copy, Clone)]
pub enum OrbCapabilities {
    CellularAndWifi,
    WifiOnly,
}

impl OrbCapabilities {
    pub async fn from_sysfs(sysfs: impl AsRef<Path>) -> Self {
        let sysfs = sysfs.as_ref().join("class").join("net").join("wwan0");
        if fs::metadata(&sysfs).await.is_ok() {
            OrbCapabilities::CellularAndWifi
        } else {
            OrbCapabilities::WifiOnly
        }
    }
}

pub const ENV_FORK_MARKER: &str = "ORB_CONND_FORK_MARKER";

// TODO: Instead of toplevel enum, use inventory crate to register entry points and an
// init() hook at entry point of program.
/// The complete set of worker entrypoints that could be executed instead of the regular `main`.
#[derive(Debug, FromPrimitive, ToPrimitive)]
#[repr(u8)]
pub enum EntryPoint {
    SecureStorage = 1,
}

impl EntryPoint {
    pub fn run(self) -> Result<()> {
        let rt = tokio::runtime::Builder::new_current_thread().build()?;
        // TODO(@vmenge): Have a way to control whether we use in-memory or actual
        // optee via runtime configuration (for testing and portability)
        let mut in_memory_ctx =
            orb_secure_storage::in_memory::InMemoryContext::default();
        rt.block_on(match self {
            EntryPoint::SecureStorage => {
                crate::secure_storage::subprocess::entry::<InMemoryBackend>(
                    tokio::io::join(tokio::io::stdin(), tokio::io::stdout()),
                    &mut in_memory_ctx,
                )
            }
        })
    }
}

impl FromStr for EntryPoint {
    type Err = eyre::Report;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_u8(u8::from_str(s).wrap_err("not a u8")?).ok_or_eyre("unknown id")
    }
}

pub fn maybe_fork() -> Result<()> {
    match std::env::var(ENV_FORK_MARKER) {
        Err(VarError::NotUnicode(_)) => panic!("expected unicode env var value"),
        Err(VarError::NotPresent) => Ok(()),
        Ok(s) => EntryPoint::from_str(&s).expect("unknown entrypoint").run(),
    }
}
