use std::{
    fs::File,
    io::{self, IsTerminal as _, Stdout, Write},
    path::Path,
};

use color_eyre::eyre::{bail, Context as _};

#[derive(Debug, derive_more::From)]
pub enum FileOrStdout {
    Stdout(Stdout),
    File(File),
}

impl FileOrStdout {
    fn either_mut<T>(
        &mut self,
        stdout: impl FnOnce(&mut Stdout) -> T,
        file: impl FnOnce(&mut File) -> T,
    ) -> T {
        match self {
            FileOrStdout::Stdout(inner) => stdout(inner),
            FileOrStdout::File(inner) => file(inner),
        }
    }
}

impl Write for FileOrStdout {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.either_mut(|w| w.write(buf), |w| w.write(buf))
    }

    fn flush(&mut self) -> io::Result<()> {
        self.either_mut(|w| w.flush(), |w| w.flush())
    }

    fn write_vectored(&mut self, bufs: &[io::IoSlice<'_>]) -> io::Result<usize> {
        self.either_mut(|w| w.write_vectored(bufs), |w| w.write_vectored(bufs))
    }

    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.either_mut(|w| w.write_all(buf), |w| w.write_all(buf))
    }

    fn write_fmt(&mut self, fmt: std::fmt::Arguments<'_>) -> io::Result<()> {
        self.either_mut(|w| w.write_fmt(fmt), |w| w.write_fmt(fmt))
    }
}

pub fn stdout_if_none(
    maybe_file: Option<&Path>,
    overwrite_if_existing: bool,
) -> color_eyre::Result<FileOrStdout> {
    let Some(path) = maybe_file else {
        let stdout = io::stdout();
        if stdout.is_terminal() {
            bail!("stdout requested but we refuse to do so when we are a tty!")
        }
        return Ok(FileOrStdout::Stdout(stdout));
    };

    let out_file = if overwrite_if_existing {
        File::options()
            .create(true)
            .truncate(true)
            .write(true)
            .open(path)
            .wrap_err("failed to create file for output")?
    } else {
        File::create_new(path).wrap_err("failed to create new file for output")?
    };

    Ok(FileOrStdout::File(out_file))
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_stdout_if_none() {
        for overwrite_if_exists in [true, false] {
            let result = stdout_if_none(None, overwrite_if_exists);
            if std::io::stdout().is_terminal() {
                assert!(
                    result.is_err(),
                    "`None` should produce Err when stdout is tty"
                );
            } else {
                assert!(
                    matches!(
                        result.expect("should never error when stdout is not tty"),
                        FileOrStdout::Stdout(_)
                    ),
                    "`None` should produce stdout when stdout is not a tty"
                );
            }
        }
    }

    #[test]
    fn test_no_file_causes_file_created() {
        for overwrite_if_exists in [true, false] {
            let tmpdir = tempfile::tempdir().unwrap();
            let out_path = tmpdir.path().join("out_file");
            assert!(!out_path.exists(), "initially file shouldn't exist");
            let writer = stdout_if_none(Some(&out_path), overwrite_if_exists)
                .expect("should never error when not `None`");
            assert!(
                matches!(writer, FileOrStdout::File(_)),
                "`None` should produce stdout when stdout is not a tty"
            );
            assert!(out_path.exists(), "afterwards, file should exist");
        }
    }
}
