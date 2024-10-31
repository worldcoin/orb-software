use super::{Agent, Kill};
use crate::port;
use futures::prelude::*;
use std::fmt::Debug;
use tokio::task;

/// Agent running on a dedicated asynchronous task.
pub trait Task: Agent + Send {
    /// Error type returned by the agent.
    type Error: Debug;

    /// Runs the agent event-loop inside a dedicated asynchronous task.
    fn run(
        self,
        port: port::Inner<Self>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Spawns a new task running the agent event-loop and returns a handle for
    /// bi-directional communication with the agent.
    fn spawn_task(self) -> (port::Outer<Self>, Kill) {
        let (inner, outer) = port::new();
        task::spawn(async move {
            tracing::info!("Agent {} spawned", Self::NAME);
            match self.run(inner).await {
                Ok(()) => {
                    tracing::warn!("Task agent {} exited", Self::NAME);
                }
                Err(err) => {
                    tracing::error!(
                        "Task agent {} exited with error: {err:#?}",
                        Self::NAME
                    );
                }
            }
        });
        (outer, future::pending().boxed())
    }
}
