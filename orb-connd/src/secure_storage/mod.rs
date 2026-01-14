mod messages;
pub(crate) mod subprocess;

use std::sync::Arc;

use self::messages::{Request, Response};
use color_eyre::eyre::{Context, Result};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::{CancellationToken, DropGuard};

type RequestChannelPayload = (Request, oneshot::Sender<Response>);

/// The effective user id for the CA.
const CA_EUID: u32 = 1000; // TODO: Figure this out

/// Async-friendly handle through which the secure storage can be talked to.
///
/// Kills it on Drop.
#[derive(Debug, Clone)]
pub struct SecureStorage {
    request_tx: mpsc::Sender<RequestChannelPayload>,
    _drop_guard: Arc<DropGuard>,
}

#[expect(dead_code)]
impl SecureStorage {
    pub fn new(cancel: CancellationToken) -> Self {
        self::subprocess::spawn(1, cancel)
    }

    pub async fn get(&self, key: String) -> Result<Option<Vec<u8>>> {
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

    pub async fn put(&self, key: String, value: Vec<u8>) -> Result<Option<Vec<u8>>> {
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
