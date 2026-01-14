mod messages;

pub mod subprocess;

use self::messages::{Request, Response};
use color_eyre::eyre::{Context, Result};
use orb_secure_storage_ca::StorageDomain;
use std::{fmt::Display, path::PathBuf, sync::Arc};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::{CancellationToken, DropGuard};

type RequestChannelPayload = (Request, oneshot::Sender<Response>);

/// The complete list of all "use cases" that connd has for storage. Each one gets
/// mapped to a different UID and/or TA.
#[derive(Debug, Eq, PartialEq, Clone, Copy, clap::ValueEnum)]
pub enum ConndStorageScopes {
    /// NetworkManager Wifi profiles
    NmProfiles,
}

impl Display for ConndStorageScopes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            ConndStorageScopes::NmProfiles => "nm-profiles",
        };

        f.write_str(s)
    }
}

impl ConndStorageScopes {
    /// The linux username that should be used for this scope
    const fn as_username(&self) -> &'static str {
        match self {
            Self::NmProfiles => "orb-ss-connd-nmprofiles",
        }
    }

    /// The linux group that should be used for this scope"
    const fn as_groupname(&self) -> &'static str {
        "worldcoin"
    }

    /// The TA storage domain that should be used when interacting with this scope.
    const fn as_domain(&self) -> StorageDomain {
        match self {
            ConndStorageScopes::NmProfiles => StorageDomain::WifiProfiles,
        }
    }
}

/// Async-friendly handle through which the secure storage can be talked to.
///
/// Kills it on Drop.
#[derive(Debug, Clone)]
pub struct SecureStorage {
    request_tx: mpsc::Sender<RequestChannelPayload>,
    _drop_guard: Arc<DropGuard>,
}

impl SecureStorage {
    pub fn new(
        exe_path: PathBuf,
        in_memory: bool,
        cancel: CancellationToken,
        scope: ConndStorageScopes,
    ) -> Self {
        self::subprocess::spawn(exe_path, in_memory, 1, cancel, scope)
    }

    pub async fn get(&self, key: String) -> Result<Vec<u8>> {
        let (response_tx, response_rx) = oneshot::channel();
        let request = Request::Get { key };
        self.request_tx
            .send((request, response_tx))
            .await
            .wrap_err("failed because the backend was killed")?;

        let response = response_rx
            .await
            .wrap_err("got an error from the backend")?;
        let Response::Get(response) = response else {
            unreachable!()
        };

        response.wrap_err("got an error from the backend")
    }

    pub async fn put(&self, key: String, value: Vec<u8>) -> Result<Vec<u8>> {
        let (response_tx, response_rx) = oneshot::channel();
        let request = Request::Put { key, val: value };
        self.request_tx
            .send((request, response_tx))
            .await
            .wrap_err("failed because the backend was killed")?;

        let response = response_rx
            .await
            .wrap_err("got an error from the backend")?;
        let Response::Put(response) = response else {
            unreachable!()
        };

        response.wrap_err("got an error from the backend")
    }
}
