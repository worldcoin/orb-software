use super::{Agent, Kill};
use crate::{port, spawn_named_thread};
use futures::prelude::*;
use std::{fmt::Debug, future, io};

/// Agent running on a dedicated OS thread.
pub trait Thread: Agent + Send {
    /// Error type returned by the agent.
    type Error: Debug;

    /// Runs the agent event-loop inside a dedicated OS thread.
    fn run(self, port: port::Inner<Self>) -> Result<(), Self::Error>;

    /// Spawns a new thread running the agent event-loop and returns a handle for
    /// bi-directional communication with the agent.
    fn spawn_thread(self) -> io::Result<(port::Outer<Self>, Kill)> {
        let (inner, outer) = port::new();
        spawn_named_thread(format!("thrd-{}", Self::NAME), move || {
            tracing::info!("Agent {} spawned", Self::NAME);
            match self.run(inner) {
                Ok(()) => {
                    tracing::warn!("Thread agent {} exited", Self::NAME);
                }
                Err(err) => {
                    tracing::error!("Thread agent {} exited with error: {err:#?}", Self::NAME);
                }
            }
        });
        Ok((outer, future::pending().boxed()))
    }
}
