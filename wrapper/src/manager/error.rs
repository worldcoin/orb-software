use std::sync::MutexGuard;

use crate::ErrorCode;

use super::Cameras;

pub(super) type Result<T> = std::result::Result<T, ManagerError>;

#[derive(thiserror::Error, Debug)]
pub enum ManagerError {
    /// An error due to poisoning of the [`super::Manager`]
    #[error("manager was poisoned")]
    PoisonError,
    /// An error returned from the SDK
    #[error(transparent)]
    SdkError(#[from] ErrorCode),
}

impl From<std::sync::PoisonError<MutexGuard<'_, Cameras>>> for ManagerError {
    fn from(_: std::sync::PoisonError<MutexGuard<'_, Cameras>>) -> Self {
        ManagerError::PoisonError
    }
}
