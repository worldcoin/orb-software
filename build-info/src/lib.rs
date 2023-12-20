//! Be sure that you run [`build_info_helper::initialize()`] in your build.rs.
#![no_std]

#[cfg(feature = "build-script")]
pub use build_info_helper::*;

// Must be the same as the one in build.rs
#[macro_export]
macro_rules! prefix_env {
    ($var:literal) => {
        env!(concat!("WORLDCOIN_BUILD_INFO_", $var))
    };
}

/// Information about the build.
pub struct BuildInfo {
    pub git: GitInfo,
}

/// Information from git
pub struct GitInfo {
    /// The result of `git describe --always --dirty=-modified`.
    pub describe: &'static str,
}

/// Calling this returns an instance of [`BuildInfo`].
///
/// Be sure that you also call [`initialize`] in your build.rs.
#[macro_export]
macro_rules! make_build_info {
    () => {{
        $crate::BuildInfo {
            git: $crate::GitInfo {
                describe: $crate::prefix_env!("GIT_DESCRIBE"),
            },
        }
    }};
}
