#![allow(dead_code)]
use async_tempfile::TempDir;
use std::path::PathBuf;
use test_utils::docker::{self, Container};

pub type Port = u16;

pub async fn run() -> (Container, Port) {
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let docker_ctx = crate_dir.join("tests");
    let dockerfile = docker_ctx.join("Dockerfile");
    let tag = "worldcoin-zenorb";

    docker::build(tag, dockerfile, docker_ctx).await;

    let port = portpicker::pick_unused_port().expect("No ports free");
    let container = docker::run_with(
        tag,
        [&format!("-p={port}:7447")],
        &TempDir::new().await.unwrap(),
    )
    .await;

    (container, port)
}
