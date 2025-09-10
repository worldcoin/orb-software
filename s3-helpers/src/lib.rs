mod client;
mod download;
mod list_prefix;
mod s3_uri;
mod upload;

pub use crate::client::{client, ClientExt};
pub use crate::download::Progress;
pub use crate::s3_uri::S3Uri;
pub use crate::upload::UploadProgress;

/// Whether to overwrite existing files or error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExistingFileBehavior {
    /// If a file exists, overwrite it
    Overwrite,
    /// If a file exists, abort
    Abort,
}

/// Whether to overwrite an existing S3 object or error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExistingObjectBehavior {
    /// If an object exists, overwrite it
    Overwrite,
    /// If an object exists, abort
    Abort,
}
