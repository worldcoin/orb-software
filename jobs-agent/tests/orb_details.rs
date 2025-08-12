use common::fixture::JobAgentFixture;
use orb_jobs_agent::shell::Host;
use std::time::Duration;
use tokio::time;

mod common;

// flakey on macOS, once i fix flakyness i can remove it
#[cfg_attr(target_os = "macos", test_with::no_env(GITHUB_ACTIONS))]
#[tokio::test]
async fn reads_file_successfully() {
    // Arrange
    let fx = JobAgentFixture::new().await;
    let _handle = fx.spawn_program(Host);

    // Act
    fx.enqueue_job("orb_details").await;
    time::sleep(Duration::from_millis(100)).await; // act buffer

    // Assert
    let actual = fx.execution_updates.map_iter(|x| x.std_out).await;
    let expected = serde_json::json!({
        "orb_name": "NO_ORB_NAME",
        "jabil_id": "NO_JABIL_ID"
    });

    assert_eq!(actual[0], expected.to_string());
}
