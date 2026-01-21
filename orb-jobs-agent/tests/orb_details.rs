use common::fixture::JobAgentFixture;
use orb_jobs_agent::shell::Host;

mod common;

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn it_reads_orb_details_successfully() {
    // Arrange
    let fx = JobAgentFixture::new().await;
    fx.program().shell(Host).spawn().await;

    // Act
    fx.enqueue_job("orb_details")
        .await
        .wait_for_completion()
        .await;

    // Assert
    let actual = fx.execution_updates.map_iter(|x| x.std_out).await;
    let expected = serde_json::json!({
        "orb_name": "NO_ORB_NAME",
        "jabil_id": "NO_JABIL_ID"
    });

    assert_eq!(actual[0], expected.to_string());
}
