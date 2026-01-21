use async_tempfile::TempFile;
use common::fixture::JobAgentFixture;
use orb_jobs_agent::shell::Host;
use tokio::fs;

mod common;

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn reads_file_successfully() {
    // Arrange
    let contents = "wubalubadubdub";
    let file = TempFile::new().await.unwrap();
    let filepath = file.file_path().to_string_lossy().to_string();
    fs::write(&filepath, &contents).await.unwrap();

    let fx = JobAgentFixture::new().await;
    fx.program().shell(Host).spawn().await;

    // Act
    fx.enqueue_job(format!("read_file {filepath}"))
        .await
        .wait_for_completion()
        .await;

    // Assert
    let result = fx.execution_updates.map_iter(|x| x.std_out).await;
    assert_eq!(result[0], contents);
}
