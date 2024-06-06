use std::{any::Any, task::Poll, time::Duration};

use pin_project::pin_project;
use tokio::sync::oneshot;

pub mod canfd;
pub mod isotp;

const ACK_RX_TIMEOUT: Duration = Duration::from_millis(1500);

pub type CanTaskResult = Result<(), CanTaskJoinError>;

/// Handle that can be used to detect errors in the can receive task.
///
/// Note that dropping this handle doesn't kill the task.
#[pin_project]
#[derive(Debug)]
pub struct CanTaskHandle(#[pin] oneshot::Receiver<CanTaskResult>);

impl std::future::Future for CanTaskHandle {
    type Output = CanTaskResult;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Self::Output> {
        let rx = self.project().0;
        rx.poll(cx).map(|recv| match recv {
            Ok(Ok(())) | Err(oneshot::error::RecvError { .. }) => Ok(()),
            Ok(Err(err)) => Err(err),
        })
    }
}

impl CanTaskHandle {
    /// Blocks until the task is complete.
    ///
    /// It is recommended to simply .await instead, since `CanTaskHandle` implements
    /// `Future`.
    pub fn join(self) -> CanTaskResult {
        match self.0.blocking_recv() {
            Ok(Ok(())) | Err(oneshot::error::RecvError { .. }) => Ok(()),
            Ok(Err(err)) => Err(err),
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum CanTaskJoinError {
    #[error(transparent)]
    Panic(#[from] CanTaskPanic),
    #[error(transparent)]
    Err(#[from] color_eyre::Report),
}

#[derive(thiserror::Error)]
#[error("panic in thread used to receive from canbus")]
// Mutex is there to make it implement Sync without using `unsafe`
pub struct CanTaskPanic(std::sync::Mutex<Box<dyn Any + Send + 'static>>);

impl CanTaskPanic {
    fn new(err: Box<dyn Any + Send + 'static>) -> Self {
        Self(std::sync::Mutex::new(err))
    }

    /// Returns the object with which the task panicked.
    ///
    /// You can pass this into [`std::panic::resume_unwind()`] to propagate the
    /// panic.
    pub fn into_panic(self) -> Box<dyn Any + Send + 'static> {
        self.0.into_inner().expect("infallible")
    }
}

impl std::fmt::Debug for CanTaskPanic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple(std::any::type_name::<CanTaskPanic>())
            .finish()
    }
}
