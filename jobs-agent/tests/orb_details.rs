use fixture::JobAgentFixture;
use orb_jobs_agent::{
    program::{self, Deps},
    shell::Host,
};
use std::time::Duration;
use tokio::{task, time};

mod fixture;

#[tokio::test]
async fn reads_file_successfully() {
    // Arrange
    let fx = JobAgentFixture::new("aaaaaaaa", "fleet-cmdr", "namespace").await;
    let _ = fx.init_tracing();

    let deps = Deps {
        shell: Box::new(Host),
        settings: fx.settings.clone(),
    };

    task::spawn(program::run(deps));

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
