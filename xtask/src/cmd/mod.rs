pub mod build;
pub mod deb;
pub mod deploy;
pub mod pre_commit;
pub mod test;

use std::process::{Command, Stdio};

use color_eyre::{eyre::eyre, Result};

pub(crate) fn cmd(args: &[&str]) -> Result<()> {
    let (program, rest) = args.split_first().ok_or_else(|| eyre!("empty cmd"))?;
    let mut command = Command::new(program);
    command.args(rest);
    command
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    let status = command.status()?;
    if !status.success() {
        return Err(eyre!("{program} exited with {status}"));
    }

    Ok(())
}
