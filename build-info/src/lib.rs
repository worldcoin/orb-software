#![no_std]

// Must be the same as the one in build.rs
macro_rules! prefix_env {
    ($var:literal) => {
        env!(concat!(
            "WORLDCOIN_BUILD_INFO_",
            env!("CARGO_PKG_VERSION"),
            "_",
            $var
        ))
    };
}

/// Information about the build.
pub struct BuildInfo {
    pub git: GitInfo,
}

impl BuildInfo {
    pub const fn new() -> Self {
        Self {
            git: GitInfo::new(),
        }
    }
}

/// Information from git
pub struct GitInfo {
    /// The result of `git describe --always --dirty=-modified`.
    pub describe: &'static str,
}

impl GitInfo {
    pub const fn new() -> Self {
        Self {
            describe: prefix_env!("GIT_DESCRIBE"),
        }
    }
}
