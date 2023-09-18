use color_eyre::{eyre::WrapErr, Help, Result};
use std::process::Command;

fn main() -> Result<()> {
    color_eyre::install()?;
    println!("cargo:rerun-if-changed=.git/HEAD");
    let git_version = if let Ok(git_version) = std::env::var("GIT_VERSION") {
        git_version
    } else {
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
        .to_string()
    };
    println!("cargo:rustc-env=GIT_VERSION={git_version}");

    Ok(())
}
