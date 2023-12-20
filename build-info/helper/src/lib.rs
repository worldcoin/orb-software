use color_eyre::{eyre::WrapErr, Help, Result};
use std::process::Command;

// Must be the same as the one in lib.rs
const ENV_PREFIX: &str = "WORLDCOIN_BUILD_INFO_";

/// Call this from within your build script.
pub fn initialize() -> Result<()> {
    color_eyre::install()?;
    println!("cargo:rerun-if-changed=.git/HEAD");

    let git_describe = read_env("GIT_DESCRIBE").unwrap_or(
        std::str::from_utf8(
            &Command::new("git")
                .arg("describe")
                .arg("--always")
                .arg("--dirty=-modified")
                .output()
                .wrap_err("Failed to run `git describe`")
                .suggestion("Is `git` installed?")?
                .stdout,
        )?
        .trim_end()
        .to_string(),
    );
    set_env("GIT_DESCRIBE", &git_describe);

    Ok(())
}

fn read_env(var: &str) -> Option<String> {
    let var = format!("{ENV_PREFIX}{var}");
    println!("cargo:rerun-if-env-changed={var}");
    match std::env::var(var) {
        Ok(s) => Some(s),
        Err(std::env::VarError::NotPresent) => None,
        Err(err) => panic!("{}", err),
    }
}

fn set_env(var: &str, value: &str) {
    println!("cargo:rustc-env={ENV_PREFIX}{var}={value}");
}
