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
    let git_path = workspace_dir().join(".git");
    let git_head_path = git_path.join("HEAD");
    let git_index_path = git_path.join("index");
    for p in [git_head_path, git_index_path] {
        println!("cargo:rerun-if-changed={}", p.display());
    }

    let git_describe = read_env("GIT_DESCRIBE")
        .ok_or(())
        .or_else(|()| git_describe())
        .wrap_err("failed to compute value for GIT_DESCRIBE")?;
    set_env("GIT_DESCRIBE", &git_describe);

    let git_dirty = read_env("GIT_DIRTY")
        .ok_or(())
        .map(|s| match s.to_lowercase().as_str() {
            "0" => false,
            "1" => true,
            "true" => true,
            "false" => false,
            _ => panic!("unexpected value"),
        })
        .or_else(|()| git_dirty())
        .wrap_err("failed to compute value for GIT_DIRTY")?;
    set_env("GIT_DIRTY", if git_dirty { "1" } else { "0" });

    let git_rev_short = read_env("GIT_REV_SHORT")
        .ok_or(())
        .or_else(|()| git_current_rev_short())
        .wrap_err("failed to compute value for GIT_REV_SHORT")?;
    set_env("GIT_REV_SHORT", &git_rev_short);

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
        .args(["locate-project", "--workspace", "--message-format=plain"])
        .output()
        .unwrap()
        .stdout;
    let workpace_cargo_toml_path =
        Path::new(std::str::from_utf8(&output).unwrap().trim());
    workpace_cargo_toml_path.parent().unwrap().to_path_buf()
}

fn git_describe() -> Result<String> {
    let stdout = Command::new("git")
        .args(["describe", "--always", "--dirty=-modified"])
        .output()
        .wrap_err("Failed to run `git describe --always --dirty=-modified`")
        .suggestion("Is `git` installed?")?
        .stdout;
    let cleaned_stdout = std::str::from_utf8(&stdout)?.trim();

    Ok(cleaned_stdout.to_owned())
}

/// Uses git status to determine if the repo is dirty.
fn git_dirty() -> Result<bool> {
    let stdout = Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .wrap_err("failed to run `git status --porcelain`")
        .suggestion("Is `git` installed?")?
        .stdout;
    let cleaned_stdout = std::str::from_utf8(&stdout)?.trim();
    let is_dirty = !cleaned_stdout.is_empty();

    Ok(is_dirty)
}

/// Uses git rev-parse to get the current git revision.
fn git_current_rev_short() -> Result<String> {
    let stdout = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .wrap_err("failed to run `git rev-parse --short HEAD`")
        .suggestion("Is `git` installed?`")?
        .stdout;
    let cleaned_stdout = std::str::from_utf8(&stdout)?.trim();

    Ok(cleaned_stdout.to_owned())
}
