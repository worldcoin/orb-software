mod client;
mod download;
mod list_prefix;
mod s3_url_parts;

pub use crate::client::{client, ClientExt};
pub use crate::download::Progress;
pub use crate::s3_url_parts::S3Uri;

/// Whether to overwrite existing files or error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExistingFileBehavior {
    /// If a file exists, overwrite it
    Overwrite,
    /// If a file exists, abort
    Abort,
}
