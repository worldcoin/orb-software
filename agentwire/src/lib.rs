//! A framework for asynchronous message-passing agents.
//!
//! There are three main components:
//! - **Agent** - a separate computation unit, which runs in its own isolated
//!   task, thread or process.
//! - **Broker** - a manager of agents. It is responsible for spawning agents,
//!   and for message passing between them.
//! - **Port** - a bi-directional communication channel between an agent and the
//!   broker.
//!
//! # Agent
//!
//! An agent is a computation unit that runs concurrently with other agents.
//! It is a structure that implements [`Agent`], [`Port`](port::Port) and other
//! trais, depending on whether it is a task-based agent, thread-based agent, or
//! a process-based agent.
//!
//! Each agent defines its own input, output, and error types, and a `run`
//! method that is called when the agent is started. The agent structure defines
//! its initial state.
//!
//! See [`agent`] module for more details.
//!
//! # Port
//!
//! A port is a bi-directional communication channel between an agent and the
//! broker. It has an input and an output side. The input side is used by the
//! broker to send messages to the agent, and the output side is used by the
//! agent to send messages to the broker.
//!
//! When used for a process-based agent, the port works via shared memory, and
//! the serialization/deserialization is done using the `rkyv` library.
//!
//! See [`port`] module for more details.
//!
//! # Broker
//!
//! A broker is a manager of agents. It is responsible for spawning agents,
//! handling the agent messages, and running **plans**. A broker shouldn't run
//! any computationally expensive tasks, and should act only as a router between
//! agents. The agents shouldn't be connected to each other directly, but only
//! through the broker. The broker and the agents form a **star topology**.
//!
//! ```ignore
//! use agentwire::{agent, Broker};
//!
//! #[derive(Broker)]
//! #[broker(plan = Plan, error = Error)]
//! struct MyBroker {
//!     #[agent(task)]
//!     foo: agent::Cell<Foo>,
//!     // non-agent fields can be added as well
//!     bar: String,
//! }
//!
//! // A broker can be created using the `new_broker!` macro, passing the
//! // non-agent fields as arguments.
//! let my_broker = new_my_broker!(bar: "baz".to_string());
//! ```
//!
//! See [`Broker`] macro for the full list of supported options.
//!
//! Each broker defines its own **Plan** trait, with a handler for each agent.
//!
//! ```ignore
//! // It's advised to provide a default implementation for each handler
//! trait Plan {
//!     // ...
//!
//!     fn handle_foo(
//!         &mut self,
//!         broker: &mut Broker,
//!         output: port::Output<Foo>,
//!     ) -> Result<BrokerFlow, Error> {
//!         Ok(BrokerFlow::Continue)
//!     }
//!
//!     // ...
//! }
//! ```
//!
//! A concrete plan can be defined by implementing the `Plan` trait for a
//! structure, and then calling the `Broker::run` method with the plan.
//!
//! ```ignore
//! struct MyPlan {
//!     result: Option<u32>,
//! }
//!
//! // A concrete plan can implement a subset of handlers
//! impl Plan for MyPlan {
//!     // ...
//!
//!     fn handle_foo(
//!         &mut self,
//!         _broker: &mut Broker,
//!         output: port::Output<Foo>,
//!     ) -> Result<BrokerFlow, Error> {
//!         self.result = Some(output.value);
//!         Ok(BrokerFlow::Break)
//!     }
//!
//!     // ...
//! }
//!
//! impl MyPlan {
//!     // A run method can be defined to run the broker with the plan.
//!     pub async fn run(mut self, broker: &mut Broker) -> Option<u32> {
//!         // Enable needed agents.
//!         broker.enable_foo()?;
//!         // Run the broker until `BrokerFlow::Break` is returned from one of the handlers.
//!         broker.run(&mut self).await?;
//!         // Disable unneeded agents.
//!         broker.disable_foo();
//!         // Return the result.
//!         self.result
//!     }
//! }
//! ```
//!
//! # Process-based agents
//!
//! Process-based agents are agents that run inside their own separate
//! processes. They are isolated from the broker and other agents, and can be
//! used to run untrusted or unreliable code.
//!
//! If process-based agents are used, a special initialization method should be
//! called at the beginning of the program. It will branch the program into an
//! agent process when special environment variables are set.
//!
//! ```ignore
//! use agentwire::agent::Process as _;
//!
//! // NOTE: keep track of all process-based agents here!
//! fn call_process_agent(name: &str, fd: OwnedFd) -> Result<(), Box<dyn Error>> {
//!     match name {
//!         "foo" => Foo::call(fd)?,
//!         "bar" => Bar::call(fd)?,
//!         _ => panic!("unregistered agent {name}"),
//!     }
//!     Ok(())
//! }
//!
//! fn main() {
//!     agentwire::agent::process::init(call_process_agent);
//! }
//! ```
//!
//! # Testing
//!
//! The [`test`] macro is provided to simplify testing of brokers. See the macro
//! documentation for more details.

#![warn(missing_docs, unsafe_op_in_unsafe_fn)]
#![warn(clippy::pedantic)]
#![allow(clippy::missing_errors_doc, clippy::missing_panics_doc)]

pub mod agent;
pub mod port;
pub mod testing_rt;

pub use agent::Agent;

/// A macro for creating a broker test.
///
/// # Examples
///
/// ```ignore
/// #[agentwire::test(
///   // Custom initialization method. Required if the broker has process-based
///   // agents.
///   init = init,
///   // Custom timeout in milliseconds. Defaults to 60000.
///   timeout = 10000,
/// )]
/// async fn test_foo() {
///     let mut broker = new_broker!();
///     let mut plan = MyPlan::new();
///     broker.run(&mut plan).await.unwrap();
/// }
///
/// // If the broker has process-based agents, a custom initialization method
/// // should be provided.
/// fn init() {
///     agentwire::agent::process::init(|name, fd| match name {
///         "foo" => Ok(Foo::call(fd)?),
///         _ => panic!("unregistered agent {name}"),
///     });
/// }
/// ```
pub use agentwire_macros::test;
/// A derive macro for creating a broker.
///
/// # Examples
///
/// ```ignore
/// use agentwire::{agent, Broker, BrokerFlow};
/// use futures::future::BoxFuture;
/// use std::task::{Context, Instant, Poll};
/// use thiserror::Error;
///
/// // Define the error type for the broker.
/// #[derive(Error, Debug)]
/// pub enum Error {}
///
/// // Define the plan trait for the broker.
/// pub trait Plan {
///     fn handle_foo(
///         &mut self,
///         broker: &mut Broker,
///         output: port::Output<Foo>,
///     ) -> Result<BrokerFlow, Error> {
///         Ok(BrokerFlow::Continue)
///     }
/// }
///
/// // Define the broker structure.
/// #[derive(Broker)]
/// #[broker(
///   plan = Plan, // Plan trait for the broker (required)
///   error = Error, // Error type used by the generated `run` method (required)
///   poll_extra, // Call `poll_extra` method in the generated `run` method (optional)
/// )]
/// pub struct MyBroker {
///     // Define the agents. Each agent should be annotated with the `agent`
///     // attribute, followed by the agent type (`task`, `thread`, `process`).
///     #[agent(
///       // The agent is task-based
///       task,
///       // The agent is thread-based
///       thread,
///       // The agent is process-based
///       process,
///       // The agent has a custom initialization method (instead of using
///       // `Default`)
///       init,
///       // The agent has a custom asynchronous initialization method (instead
///       // of using `Default`)
///       init_async,
///       // The process-agent has a custom logger
///       logger = self.process_logger().await,
///     )]
///     foo: agent::Cell<Foo>,
///     // non-agent fields can be added as well
///     bar: String,
/// }
///
/// impl MyBroker {
///     // Implement the `init_foo` method if the `init` option is enabled.
///     fn init_foo(&mut self) -> Foo {
///         Foo {}
///     }
///
///     // Implement the asynchronous `init_foo` method if the `init_async`
///     // option is enabled.
///     async fn init_foo(&mut self) -> Result<Foo, Error> {
///         Ok(Foo {})
///     }
///
///     // Implement the handler method for the `foo` agent.
///     fn handle_foo(
///         &mut self,
///         plan: &mut dyn Plan,
///         output: port::Output<Foo>,
///     ) -> Result<BrokerFlow, Error> {
///         plan.handle_foo(self, output)
///     }
///
///     // Implement the `poll_extra` method if it's enabled.
///     fn poll_extra(
///         &mut self,
///         plan: &mut dyn Plan,
///         cx: &mut Context<'_>,
///         fence: Instant,
///     ) -> Result<Option<Poll<()>>> {
///         Ok(Some(Poll::Pending))
///     }
///
///     // Implement a custom logger for process-based agents.
///     async fn process_logger(
///         &self,
///     ) -> impl Fn(&'static str, ChildStdout, ChildStderr) -> BoxFuture<()> + Send + 'static
///     {
///         move |agent_name, stdout, stderr| {
///             Box::pin(agentwire::agent::process::default_logger(agent_name, stdout, stderr))
///         }
///     }
/// }
///
/// // `new_my_broker!` macro is generated by the `Broker` macro. It takes the
/// // non-agent fields as arguments.
/// let my_broker = new_my_broker!(bar: "baz".to_string());
/// ```
pub use agentwire_macros::Broker;

use std::{ffi::CString, fmt::Display, io, thread};
use thiserror::Error;

/// Used to tell a broker whether it should exit early or go on as usual.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BrokerFlow {
    /// Continue managing agents.
    Continue,
    /// Stops the broker returning control to the caller.
    Break,
}

/// The type of error that can occur in a broker.
#[derive(Error, Debug)]
pub enum BrokerError<T: Display> {
    /// An agent initialization error.
    #[error("agent {0} initialization: {1}")]
    Init(&'static str, T),
    /// An agent spawning error.
    #[error("agent {0} thread spawning: {1}")]
    SpawnThread(&'static str, io::Error),
    /// An agent handler error.
    #[error("agent {0} handler: {1}")]
    Handler(&'static str, T),
    /// `poll_extra` method error.
    #[error("poll_extra: {0}")]
    PollExtra(T),
    /// An agent has terminated.
    #[error("agent {0} terminated")]
    AgentTerminated(&'static str),
}

fn spawn_named_thread<F, T>(name: impl Into<String>, f: F) -> thread::JoinHandle<T>
where
    F: FnOnce() -> T,
    F: Send + 'static,
    T: Send + 'static,
{
    let name = name.into();
    thread::Builder::new()
        .name(name.clone())
        .spawn(move || {
            if let Ok(title) = CString::new(name.as_bytes()) {
                let result =
                    unsafe { libc::prctl(libc::PR_SET_NAME, title.as_ptr(), 0, 0, 0) };
                if result == -1 {
                    eprintln!(
                        "failed to set thread name to '{name}': {:#?}",
                        io::Error::last_os_error()
                    );
                }
            }
            f()
        })
        .expect("failed to spawn thread")
}
