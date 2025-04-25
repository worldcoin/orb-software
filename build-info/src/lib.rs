//! Be sure that you run `build_info_helper::initialize()` in your build.rs.
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
#[derive(Debug, Eq, PartialEq, Clone)]
pub struct BuildInfo {
    pub git: GitInfo,
    pub cargo: CargoInfo,
    /// The user-facing version number we should report. Pass this to clap.
    pub version: &'static str,
}

/// Information from git.
#[derive(Debug, Eq, PartialEq, Clone)]
pub struct GitInfo {
    /// The result of `git describe --always --dirty=-modified`.
    pub describe: &'static str,
    /// Whether `git status --porcelain` returns anything or not.
    pub dirty: bool,
    /// The current git revision, via `git rev-parse --short HEAD`
    pub rev_short: &'static str,
}

/// Information from cargo.
#[derive(Debug, Eq, PartialEq, Clone)]
pub struct CargoInfo {
    /// The version field in Cargo.toml.
    pub pkg_version: &'static str,
}

/// Calling this returns an instance of [`BuildInfo`].
///
/// Be sure that you also call `orb_build_info::initialize()` in your build.rs.
#[macro_export]
macro_rules! make_build_info {
    () => {
        const {
            const TMP: $crate::BuildInfo = $crate::BuildInfo {
                git: $crate::GitInfo {
                    describe: $crate::prefix_env!("GIT_DESCRIBE"),
                    dirty: $crate::are_strs_equal(
                        $crate::prefix_env!("GIT_DIRTY"),
                        "1",
                    ),
                    rev_short: $crate::prefix_env!("GIT_REV_SHORT"),
                },
                cargo: $crate::CargoInfo {
                    pkg_version: env!("CARGO_PKG_VERSION"),
                },
                version: "", // will be overwritten in a moment
            };
            const DIRTY_SUFFIX: &str = if TMP.git.dirty { "-modified" } else { "" };
            let build_info = $crate::BuildInfo {
                version: $crate::const_concat!(
                    TMP.cargo.pkg_version,
                    " ",
                    TMP.git.rev_short,
                    DIRTY_SUFFIX
                ),
                ..TMP
            };
            assert!(!build_info.version.is_empty());
            build_info
        }
    };
}

#[doc(hidden)]
pub const fn are_strs_equal(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let a = a.as_bytes();
    let b = b.as_bytes();
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }

    true
}

#[cfg(test)]
mod test {
    use crate::are_strs_equal;

    #[test]
    fn test_str_equal() {
        assert!(are_strs_equal("foobar", "foobar"));
        assert!(are_strs_equal("a", "a"));
        assert!(are_strs_equal("", ""));

        assert!(!are_strs_equal("foo", "bar"));
        assert!(!are_strs_equal("foo", " foo"));
        assert!(!are_strs_equal("foo ", "foo"));
        assert!(!are_strs_equal("", " "));
    }
}
