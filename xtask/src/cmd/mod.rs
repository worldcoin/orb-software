pub mod android;
pub mod build;
pub mod deb;
pub mod deploy;
pub mod pre_commit;
pub mod test;
pub mod test_watch;

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

/// Like [`cmd`], but captures stdout/stderr instead of streaming them live -
/// for callers running several of these concurrently, where interleaved
/// output from independent processes would otherwise be unreadable. On
/// success, returns the captured output; on failure, the error includes it.
pub(crate) fn cmd_captured(args: &[&str]) -> Result<String> {
    let (program, rest) = args.split_first().ok_or_else(|| eyre!("empty cmd"))?;
    let output = Command::new(program).args(rest).output()?;

    let mut captured = String::from_utf8_lossy(&output.stdout).into_owned();
    captured.push_str(&String::from_utf8_lossy(&output.stderr));

    if !output.status.success() {
        return Err(eyre!(
            "{program} exited with {}:\n{captured}",
            output.status
        ));
    }

    Ok(captured)
}
