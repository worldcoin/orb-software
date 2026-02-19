use crate::cmd::cmd;
use color_eyre::Result;

pub fn run() -> Result<()> {
    cmd(&[
        "cargo",
        "clippy",
        "--all",
        "--all-features",
        "--all-targets",
        "--no-deps",
        "--",
        "-D",
        "warnings",
    ])?;
    cmd(&["cargo", "fmt"])?;
    cmd(&["taplo", "format"])?;

    Ok(())
}
