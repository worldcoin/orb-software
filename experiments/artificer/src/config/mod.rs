use serde::{Deserialize, Serialize};

mod lock;
pub mod sources;
mod spec;

pub use self::lock::LockedSpec;
pub use self::spec::Spec;

/// `[artifacts.<artifact-name>]`. See also, [`Artifact`].
#[derive(Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct ArtifactName(pub String);
