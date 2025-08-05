use async_trait::async_trait;
use color_eyre::Result;
use orb_jobs_agent::shell::Shell;
use std::{path::PathBuf, process::Stdio};
use testcontainers::{
    core::WaitFor, runners::AsyncRunner, ContainerAsync, GenericImage,
};
use tokio::process::{Child, Command};

#[derive(Debug)]
pub struct FakeOrb {
    container: ContainerAsync<GenericImage>,
}

impl FakeOrb {
    const IMAGE_TAG: &str = "fake-orb";

    pub fn context_dir() -> PathBuf {
        let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        crate_dir.join("tests").join("docker")
    }

    async fn build_image() {
        let context_dir = Self::context_dir();

        let mut child = Command::new("docker")
            .arg("build")
            .arg("-t")
            .arg(Self::IMAGE_TAG)
            .arg("-f")
            .arg(context_dir.join("Dockerfile"))
            .arg(context_dir)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();

        child.wait().await.unwrap();
    }

    pub async fn new() -> Self {
        Self::build_image().await;
        let _container = GenericImage::new(Self::IMAGE_TAG, "latest")
            .with_wait_for(WaitFor::Nothing)
            .start()
            .await
            .unwrap();

        Self {
            container: _container,
        }
    }
}

#[async_trait]
impl Shell for FakeOrb {
    async fn exec(&self, cmd: &[&str]) -> Result<Child> {
        let id = self.container.id();

        let child = Command::new("docker")
            .arg("exec")
            .arg(id)
            .args(cmd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        Ok(child)
    }
}
