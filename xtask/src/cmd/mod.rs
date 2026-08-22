pub mod android;
pub mod build;
pub mod deb;
pub mod deploy;
pub mod pre_commit;
pub mod test;
pub mod test_watch;

use std::process::{Command, Stdio};

use color_eyre::{eyre::eyre, Result};

fn new_command<'a>(args: &[&'a str]) -> Result<(&'a str, Command)> {
    let (program, rest) = args.split_first().ok_or_else(|| eyre!("empty cmd"))?;
    let mut command = Command::new(program);
    command.args(rest);

    Ok((program, command))
}

pub(crate) fn cmd(args: &[&str]) -> Result<()> {
    let (program, mut command) = new_command(args)?;
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
/// success, returns the captured output, stdout followed by stderr, as raw
/// bytes - the child's output isn't guaranteed to be valid UTF-8.
pub(crate) fn cmd_captured<S: AsRef<OsStr>>(args: &[S]) -> Result<Vec<u8>> {
    let (program, mut command) = new_command(args)?;
    let mut output = command.output()?;

    if !output.status.success() {
        return Err(eyre!("{program} exited with {}", output.status));
    }

    output.stdout.extend(output.stderr);

    Ok(output.stdout)
}
