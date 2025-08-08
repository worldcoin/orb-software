use async_tempfile::TempFile;
use common::fixture::JobAgentFixture;
use orb_jobs_agent::shell::Host;
use std::time::Duration;
use tokio::{fs, time};

mod common;

#[tokio::test]
async fn reads_file_successfully() {
    // Arrange
    let contents = "wubalubadubdub";
    let file = TempFile::new().await.unwrap();
    let filepath = file.file_path().to_string_lossy().to_string();
    fs::write(&filepath, &contents).await.unwrap();

    let fx = JobAgentFixture::new().await;
    fx.init_tracing();
    fx.spawn_program(Host);

    // Act
    fx.enqueue_job(format!("read_file {filepath}")).await;
    time::sleep(Duration::from_millis(500)).await; // give enough time to read file

    // Assert
    let result = fx.execution_updates.map_iter(|x| x.std_out).await;
    assert_eq!(result[0], contents);
}
