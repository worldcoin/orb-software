//! Agent module.
//!
//! # Examples
//!
//! ```
//! # #[tokio::main] async fn main() {
//! use agentwire::{
//!     agent::{self, Agent, Task as _},
//!     port::{self, Port},
//! };
//! use futures::{
//!     channel::mpsc::{self, SendError},
//!     prelude::*,
//! };
//!
//! /// An agent that receives numbers, multiplies them by 2, and sends them
//! /// back.
//! struct Doubler;
//!
//! impl Port for Doubler {
//!     type Input = u32;
//!     type Output = u32;
//!
//!     const INPUT_CAPACITY: usize = 0;
//!     const OUTPUT_CAPACITY: usize = 0;
//! }
//!
//! impl Agent for Doubler {
//!     const NAME: &'static str = "doubler";
//! }
//!
//! impl agent::Task for Doubler {
//!     type Error = SendError;
//!
//!     async fn run(self, mut port: port::Inner<Self>) -> Result<(), Self::Error> {
//!         while let Some(x) = port.next().await {
//!             port.send(x.chain(x.value * 2)).await?;
//!         }
//!         Ok(())
//!     }
//! }
//!
//! let (mut doubler, _kill) = Doubler.spawn_task();
//!
//! // Send an input message to the agent.
//! doubler.send(port::Input::new(3)).await;
//! // Receive an output message from the agent.
//! let output = doubler.next().await;
//! assert_eq!(output.unwrap().value, 6);
//! # }
//! ```

pub mod process;

mod task;
mod thread;

pub use self::{process::Process, task::Task, thread::Thread};

use crate::port::{self, Port};
use futures::prelude::*;
use std::{mem::replace, pin::Pin};

/// Abstract agent.
pub trait Agent: Port + Sized + 'static {
    /// Name of the agent. Must be unique.
    const NAME: &'static str;
}

/// Future to kill an agent.
pub type Kill = Pin<Box<dyn Future<Output = ()> + Send>>;

/// Agent cell inside a broker.
pub enum Cell<T: Agent> {
    /// Agent is not initialized.
    Vacant,
    /// Agent is initialized and enabled.
    Enabled((port::Outer<T>, Kill)),
    /// Agent is initialized but disabled.
    Disabled((port::Outer<T>, Kill)),
}

impl<T: Agent> Cell<T> {
    /// Returns `Some(port)` if the agent is enabled, otherwise returns `None`.
    pub fn enabled(&mut self) -> Option<&mut port::Outer<T>> {
        match self {
            Self::Vacant | Self::Disabled(_) => None,
            Self::Enabled((ref mut port, _kill)) => Some(port),
        }
    }

    /// Returns `true` if the agent is enabled.
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        matches!(self, Self::Enabled(_))
    }

    /// Returns `true` if the agent is initialized.
    #[must_use]
    pub fn is_initialized(&self) -> bool {
        !matches!(self, Self::Vacant)
    }

    /// Kills the agent.
    pub async fn kill(&mut self) {
        match replace(self, Self::Vacant) {
            Self::Enabled((_port, kill)) | Self::Disabled((_port, kill)) => kill.await,
            Self::Vacant => {}
        }
    }
}
