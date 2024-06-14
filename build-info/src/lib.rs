//! Be sure that you run [`build_info_helper::initialize()`] in your build.rs.
#![no_std]
#![forbid(unsafe_code)]

#[cfg(feature = "build-script")]
pub use orb_build_info_helper::*;

#[doc(hidden)]
pub use orb_const_concat::const_concat;

/// Must be the same as the one in build.rs
#[doc(hidden)]
#[macro_export]
macro_rules! prefix_env {
    ($var:literal) => {
        env!(concat!("WORLDCOIN_BUILD_INFO_", $var))
    };
}

/// Information about the build.
pub struct BuildInfo {
    pub git: GitInfo,
    pub cargo: CargoInfo,
    /// The user-facing version number we should report. Pass this to clap.
    pub version: &'static str,
}

/// Information from git.
pub struct GitInfo {
    /// The result of `git describe --always --dirty=-modified`.
    pub describe: &'static str,
}

/// Information from cargo.
pub struct CargoInfo {
    /// The version field in Cargo.toml.
    pub pkg_version: &'static str,
}

/// Calling this returns an instance of [`BuildInfo`].
///
/// Be sure that you also call [`initialize`] in your build.rs.
#[macro_export]
macro_rules! make_build_info {
    () => {
        const {
            const TMP: $crate::BuildInfo = $crate::BuildInfo {
                git: $crate::GitInfo {
                    describe: $crate::prefix_env!("GIT_DESCRIBE"),
                },
                cargo: $crate::CargoInfo {
                    pkg_version: env!("CARGO_PKG_VERSION"),
                },
                version: "", // will be overwritten in a moment
            };
            let build_info = $crate::BuildInfo {
                version: $crate::const_concat!(
                    TMP.cargo.pkg_version,
                    " ",
                    TMP.git.describe,
                ),
                ..TMP
            };
            assert!(!build_info.version.is_empty());
            build_info
        }
    };
}
