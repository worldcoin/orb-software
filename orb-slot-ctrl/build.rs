#![warn(clippy::pedantic)]

use std::{fs, path::Path, process::Command, str};

fn main() -> eyre::Result<()> {
    // Save git commit hash at compile-time as environment variable
    println!("cargo:rerun-if-changed=.git/HEAD");
    let git_commit = Path::new("git_commit");
    let git_commit = if git_commit.exists() {
        fs::read_to_string(git_commit).expect("failed to read git_commit")
    } else {
        str::from_utf8(
            &Command::new("git")
                .arg("describe")
                .arg("--always")
                .arg("--dirty=-modified")
                .output()?
                .stdout,
        )?
        .trim_end()
        .to_string()
    };
    println!("cargo:rustc-env=GIT_COMMIT={git_commit:0>4}");
    Ok(())
}
