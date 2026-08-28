use crate::status_client::SharedToken;
use std::{
    sync::{Arc, Mutex},
    thread::JoinHandle,
    time::Duration,
};
use tracing::error;

/// How often to poll `orb-attest`'s binder service for the current token.
/// `IAuthTokenManager.aidl` exposes no change notification, only a getter.
const POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Spawn a thread that polls `orb-attest`'s `IAuthTokenManager` binder
/// service for the current token, keeping the shared handle up to date.
/// Transient binder errors are logged and the last-known token is kept.
pub fn spawn(shutdown_rx: flume::Receiver<()>) -> (SharedToken, JoinHandle<()>) {
    let token: SharedToken = Arc::new(Mutex::new(String::new()));

    let shared = token.clone();
    let handle = std::thread::spawn(move || loop {
        match orb_oes_binder::get_auth_token() {
            Ok(new_token) => {
                *shared.lock().unwrap() = new_token;
            }

            Err(e) => {
                error!("failed to fetch attest token (will retry): {e:?}");
            }
        }

        match shutdown_rx.recv_timeout(POLL_INTERVAL) {
            Err(flume::RecvTimeoutError::Timeout) => {}
            Ok(()) | Err(flume::RecvTimeoutError::Disconnected) => break,
        }
    });

    (token, handle)
}
