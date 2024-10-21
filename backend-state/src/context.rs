//! Shared state/context

use color_eyre::{eyre::WrapErr, Result};
use std::sync::Arc;
use tokio::sync::watch;

use crate::{
    api::{OrbId, Token},
    state::State,
};

/// Common shared state/context. Cheaply cloneable.
#[derive(Debug, Clone)]
pub struct Context {
    pub token: watch::Receiver<Token>,
    pub state: SharedState,
    pub orb_id: OrbId,
}

impl Context {
    pub async fn new(token: watch::Receiver<Token>) -> Result<Self> {
        let orb_id = if let Ok(orb_id) = std::env::var("ORB_ID") {
            assert!(!orb_id.is_empty());
            OrbId::from(orb_id)
        } else {
            let output = tokio::process::Command::new("orb-id")
                .output()
                .await
                .wrap_err("failed to call orb-id binary")?;
            assert!(output.status.success(), "orb-id binary failed");
            String::from_utf8(output.stdout)
                .wrap_err("orb-id output was not utf8")
                .map(OrbId::from)?
        };

        Ok(Self {
            token,
            state: Default::default(),
            orb_id,
        })
    }
}

/// Cheaply cloneable, shareable version of `Option<State>`.
/// Also provides functions to wait on state changes.
#[derive(Clone, Debug)]
pub struct SharedState {
    sender: Arc<watch::Sender<Option<State>>>,
    receiver: watch::Receiver<Option<State>>,
}

impl Default for SharedState {
    fn default() -> Self {
        let (send, recv) = watch::channel(None);
        Self {
            sender: Arc::new(send),
            receiver: recv,
        }
    }
}

impl SharedState {
    /// Gets a copy of the state.
    pub fn get_cloned(&self) -> Option<State> {
        self.receiver.borrow().clone()
    }

    /// Replaces the current state with the new_state
    pub fn update(&self, new_state: State) {
        self.sender.send(Some(new_state)).expect(
            "Failed to send on watch channel, but this should be impossible.
            There is always at least one receiver",
        );
    }

    /// Waits for an update to the `SharedState`.
    pub async fn wait_for_update(&mut self) -> State {
        drop(self.receiver.borrow_and_update()); // mark as seen
        self.receiver
            .changed()
            .await
            .expect("Failed to recv on watch channel");
        self.receiver.borrow().as_ref().unwrap().clone()
    }
}
