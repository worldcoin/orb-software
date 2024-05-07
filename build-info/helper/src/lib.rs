use color_eyre::{eyre::WrapErr, Help, Result};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

// Must be the same as the one in lib.rs
const ENV_PREFIX: &str = "WORLDCOIN_BUILD_INFO_";

/// Call this from within your build script.
pub fn initialize() -> Result<()> {
    color_eyre::install().ok();
    let git_head_path = workspace_dir().join(".git").join("HEAD");
    println!("cargo:rerun-if-changed={}", git_head_path.display());

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

// https://stackoverflow.com/a/74942075
fn workspace_dir() -> PathBuf {
    let output = std::process::Command::new(env!("CARGO"))
        .arg("locate-project")
        .arg("--workspace")
        .arg("--message-format=plain")
        .output()
        .unwrap()
        .stdout;
    let workpace_cargo_toml_path =
        Path::new(std::str::from_utf8(&output).unwrap().trim());
    workpace_cargo_toml_path.parent().unwrap().to_path_buf()
}
