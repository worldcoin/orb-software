pub mod build;
pub mod deb;
pub mod deploy;
pub mod pre_commit;
pub mod test;
pub mod test_watch;

use std::ffi::OsStr;
use std::process::{Command, Stdio};

use color_eyre::{eyre::eyre, Result};

/// Builds a `[&OsStr; N]` from a mix of `&str`/`&String`/`&Path`/`&PathBuf`
/// arguments - so a `cmd(&args![...])` call site can freely mix string
/// literals and paths in one argument list.
macro_rules! args {
    ($($arg:expr),+ $(,)?) => {
        [$(::std::convert::AsRef::<::std::ffi::OsStr>::as_ref($arg)),+]
    };
}
pub(crate) use args;

fn new_command<S: AsRef<OsStr>>(args: &[S]) -> Result<Command> {
    let (program, rest) = args.split_first().ok_or_else(|| eyre!("empty cmd"))?;
    let mut command = Command::new(program);
    command.args(rest);
    // Force a consistent, unlocalized locale, so callers that pattern-match
    // on a command's output text (e.g. android.rs's adb error checks)
    // aren't broken by a localized message on some other machine.
    command.env("LC_ALL", "C");

    Ok(command)
}

pub(crate) fn cmd<S: AsRef<OsStr>>(args: &[S]) -> Result<()> {
    let mut command = new_command(args)?;
    command
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    let status = command.status()?;
    if !status.success() {
        let program = command.get_program();
        return Err(eyre!("{} exited with {status}", program.display()));
    }

    Ok(())
}

/// Like [`cmd`], but captures stdout/stderr instead of streaming them live -
/// for callers running several of these concurrently (where interleaved
/// output from independent processes would otherwise be unreadable), or
/// that need to inspect *why* a command failed instead of just that it
/// did. Returns [`std::process::Output`] as-is, regardless of exit status.
pub(crate) fn cmd_captured<S: AsRef<OsStr>>(
    args: &[S],
) -> Result<std::process::Output> {
    let mut command = new_command(args)?;

    Ok(command.output()?)
}
