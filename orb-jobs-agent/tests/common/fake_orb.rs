#![allow(dead_code)]
use async_trait::async_trait;
use color_eyre::Result;
use orb_jobs_agent::shell::Shell;
use std::{path::PathBuf, process::Stdio, time::Duration};
use tokio::{
    process::{Child, Command},
    time,
};

/// Starts a container with stub binaries to test `orb-jobs-agent` commands with.
#[derive(Debug)]
pub struct FakeOrb {
    container_id: String,
    engine: String,
}

impl FakeOrb {
    const IMAGE_TAG: &str = "fake-orb";

    pub fn context_dir() -> PathBuf {
        let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        crate_dir.join("tests").join("docker")
    }

    async fn detect_engine() -> String {
        if let Ok(engine) = std::env::var("ORB_CONTAINER_ENGINE") {
            return engine;
        }

        // Check if docker is available
        if Command::new("docker")
            .arg("-v")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .is_ok()
        {
            "docker".to_string()
        } else {
            // Fallback to podman
            "podman".to_string()
        }
    }

    async fn build_image(engine: &str) {
        let context_dir = Self::context_dir();

        let mut child = Command::new(engine)
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
        let engine = Self::detect_engine().await;
        Self::build_image(&engine).await;

        let out = Command::new(&engine)
            .args(["run", "-d", "--rm", Self::IMAGE_TAG])
            .output()
            .await
            .unwrap();

        if !out.status.success() {
            panic!("{}", String::from_utf8_lossy(&out.stderr));
        }

        let container_id = String::from_utf8(out.stdout).unwrap();

        time::sleep(Duration::from_millis(500)).await;

        Self {
            container_id,
            engine,
        }
    }
}

#[async_trait]
impl Shell for FakeOrb {
    async fn exec(&self, cmd: &[&str]) -> Result<Child> {
        let child = Command::new(&self.engine)
            .arg("exec")
            .arg(&self.container_id)
            .args(cmd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        Ok(child)
    }
}
