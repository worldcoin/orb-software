use color_eyre::eyre::{ContextCompat, Result};
use orb_relay_messages::tonic::async_trait;
use std::{fmt, process::Stdio};
use tokio::process::Child;

#[async_trait]
pub trait Shell: Send + Sync + fmt::Debug {
    async fn exec(&self, cmd: &[&str]) -> Result<Child>;
}

#[derive(Debug)]
pub struct Host;

#[async_trait]
impl Shell for Host {
    async fn exec(&self, cmd: &[&str]) -> Result<Child> {
        let (cmd, args) = cmd.split_first().wrap_err("'cmd' arg cannot be empty")?;
        let child = tokio::process::Command::new(cmd)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        Ok(child)
    }
}
