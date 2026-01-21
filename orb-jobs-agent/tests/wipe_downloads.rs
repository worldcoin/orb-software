use common::fixture::JobAgentFixture;
use orb_jobs_agent::shell::Host;
use orb_relay_messages::jobs::v1::JobExecutionStatus;
use tokio::fs;

mod common;

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn deletes_mixed_files_and_directories() {
    // Arrange
    let mut fx = JobAgentFixture::new().await;
    let downloads_dir = fx.settings.store_path.join("downloads");
    fs::create_dir_all(&downloads_dir).await.unwrap();
    fx.settings.downloads_path = downloads_dir.clone();

    fs::write(downloads_dir.join("file1.txt"), "content1")
        .await
        .unwrap();
    fs::write(downloads_dir.join("file2.bin"), "content2")
        .await
        .unwrap();
    let subdir = downloads_dir.join("subdir");
    fs::create_dir_all(&subdir).await.unwrap();
    fs::write(subdir.join("nested.txt"), "nested")
        .await
        .unwrap();

    // Act
    fx.program().shell(Host).spawn().await;
    fx.enqueue_job("wipe_downloads".to_string())
        .await
        .wait_for_completion()
        .await;

    // Assert
    assert!(!downloads_dir.join("file1.txt").exists());
    assert!(!downloads_dir.join("file2.bin").exists());
    assert!(!subdir.exists());

    let mut entries = fs::read_dir(&downloads_dir).await.unwrap();
    let mut count = 0;
    while entries.next_entry().await.unwrap().is_some() {
        count += 1;
    }
    assert_eq!(count, 0);

    let result = fx.execution_updates.map_iter(|x| x.std_out).await;
    assert!(result[0].contains("Deleted 3"));
    assert!(result[0].contains("Failed 0"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn handles_nonexistent_directory() {
    // Arrange
    let fx = JobAgentFixture::new().await;

    // Act
    fx.program().shell(Host).spawn().await;
    fx.enqueue_job("wipe_downloads".to_string())
        .await
        .wait_for_completion()
        .await;

    // Assert
    let status = fx.execution_updates.map_iter(|x| x.status).await;
    assert_eq!(
        status.last().unwrap(),
        &(JobExecutionStatus::Succeeded as i32)
    );

    let result = fx.execution_updates.map_iter(|x| x.std_out).await;
    assert!(result[0].contains("does not exist"));
    assert!(result[0].contains("nothing to delete"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn handles_empty_directory() {
    // Arrange
    let mut fx = JobAgentFixture::new().await;
    let downloads_dir = fx.settings.store_path.join("downloads");
    fs::create_dir_all(&downloads_dir).await.unwrap();
    fx.settings.downloads_path = downloads_dir.clone();

    // Act
    fx.program().shell(Host).spawn().await;
    fx.enqueue_job("wipe_downloads".to_string())
        .await
        .wait_for_completion()
        .await;

    // Assert
    let status = fx.execution_updates.map_iter(|x| x.status).await;
    assert_eq!(
        status.last().unwrap(),
        &(JobExecutionStatus::Succeeded as i32)
    );

    let result = fx.execution_updates.map_iter(|x| x.std_out).await;
    assert!(result[0].contains("Deleted 0"));
    assert!(result[0].contains("Failed 0"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn handles_cancellation() {
    // Arrange
    let mut fx = JobAgentFixture::new().await;
    let downloads_dir = fx.settings.store_path.join("downloads");
    fs::create_dir_all(&downloads_dir).await.unwrap();
    fx.settings.downloads_path = downloads_dir.clone();

    for i in 0..100 {
        fs::write(
            downloads_dir.join(format!("file{i}.txt")),
            format!("content{i}"),
        )
        .await
        .unwrap();
    }

    // Act
    fx.program().shell(Host).spawn().await;
    let ticket = fx.enqueue_job("wipe_downloads".to_string()).await;
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    fx.cancel_job(&ticket.exec_id).await;
    ticket.wait_for_completion().await;

    // Assert
    let status = fx.execution_updates.map_iter(|x| x.status).await;
    let final_status = status.last().unwrap();

    let is_cancelled = final_status == &(JobExecutionStatus::Cancelled as i32);
    let is_succeeded = final_status == &(JobExecutionStatus::Succeeded as i32);
    assert!(
        is_cancelled || is_succeeded,
        "Expected Cancelled or Succeeded status, got: {final_status}",
    );

    let result = fx.execution_updates.map_iter(|x| x.std_out).await;
    if is_cancelled {
        let has_cancellation_msg = result.iter().any(|s| s.contains("cancelled"));
        assert!(has_cancellation_msg, "Expected cancellation message");
    }
}
