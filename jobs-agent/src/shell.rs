use color_eyre::eyre::{ContextCompat, Result};
use std::process::Stdio;
use tokio::process::Child;

pub trait Shell {
    async fn run(&self, cmd: &[&str]) -> Result<Child>;
}

pub struct Host;

impl Shell for Host {
    async fn run(&self, cmd: &[&str]) -> Result<Child> {
        let (cmd, args) = cmd.split_first().wrap_err("'cmd' arg cannot be empty")?;
        let child = tokio::process::Command::new(cmd)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        Ok(child)
    }
}
