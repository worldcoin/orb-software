use async_tempfile::TempFile;
use common::fixture::JobAgentFixture;
use orb_jobs_agent::shell::Host;
use orb_relay_messages::jobs::v1::JobExecutionStatus;
use tokio::fs;

mod common;

#[tokio::test(flavor = "multi_thread")]
async fn it_changes_name_successfully() {
    // Arrange
    let temp_file = TempFile::new().await.unwrap();
    let filepath = temp_file.file_path().to_path_buf();

    let mut fx = JobAgentFixture::new().await;
    fx.settings.orb_name_path = filepath.clone();
    fx.program().shell(Host).spawn().await;

    // Act
    fx.enqueue_job("change_name test-orb".to_string())
        .await
        .wait_for_completion()
        .await;

    // Assert
    let status = fx.execution_updates.map_iter(|x| x.status).await;
    assert_eq!(
        status.last().unwrap(),
        &(JobExecutionStatus::Succeeded as i32)
    );

    let contents = fs::read_to_string(&filepath).await.unwrap();
    assert_eq!(contents, "test-orb");

    let result = fx.execution_updates.map_iter(|x| x.std_out).await;
    assert!(result[0].contains("test-orb"));
}

#[tokio::test(flavor = "multi_thread")]
async fn it_validates_dash_requirement() {
    // Arrange
    let temp_file = TempFile::new().await.unwrap();
    let filepath = temp_file.file_path().to_path_buf();
    fs::remove_file(&filepath).await.ok();

    let mut fx = JobAgentFixture::new().await;
    fx.settings.orb_name_path = filepath.clone();
    fx.program().shell(Host).spawn().await;

    // Act
    fx.enqueue_job("change_name nodash".to_string())
        .await
        .wait_for_completion()
        .await;

    // Assert
    let status = fx.execution_updates.map_iter(|x| x.status).await;
    assert_eq!(status.last().unwrap(), &(JobExecutionStatus::Failed as i32));
}
