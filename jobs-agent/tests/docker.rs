use async_trait::async_trait;
use color_eyre::{eyre::bail, Result};
use orb_jobs_agent::shell::Shell;
use std::{path::PathBuf, process::Stdio};
use testcontainers::{ContainerAsync, GenericImage};
use tokio::process::{Child, Command};

#[derive(Debug)]
pub struct Docker {
    _container: ContainerAsync<GenericImage>,
}

impl Docker {
    pub async fn build() -> Result<()> {
        let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let context_dir = crate_dir.join("tests").join("docker");

        let output = Command::new("docker")
            .arg("build")
            .arg("-t")
            .arg("fake-orb")
            .arg("-f")
            .arg(context_dir.join("Dockerfile"))
            .arg(context_dir)
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            bail!(stderr);
        }

        Ok(())
    }
}

#[async_trait]
impl Shell for Docker {
    async fn exec(&self, cmd: &[&str]) -> Result<Child> {
        let child = Command::new("")
            .args(cmd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        Ok(child)
    }
}
