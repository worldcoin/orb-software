use common::fixture::JobAgentFixture;
use orb_jobs_agent::{
    job_system::{ctx::JobExecutionUpdateExt, handler::JobHandler},
    program::Deps,
    shell::Host,
};
use std::time::Duration;
use tokio::{task, time};

mod common;

// flakey on macOS, once i fix flakyness i can remove it
#[cfg_attr(target_os = "macos", test_with::no_env(GITHUB_ACTIONS))]
#[tokio::test]
async fn sequential_jobs_block_other_jobs_execution() {
    // Arrange
    let fx = JobAgentFixture::new().await;
    let deps = Deps::new(Host, fx.settings.clone());

    let wait_time = Duration::from_millis(100);

    task::spawn(
        JobHandler::builder()
            .sequential("first", move |ctx| async move {
                time::sleep(wait_time).await;
                Ok(ctx.success().stdout("one"))
            })
            .parallel("second", async |ctx| Ok(ctx.success().stdout("two")))
            .build(deps)
            .run(),
    );

    // Act
    fx.enqueue_job("first").await;
    fx.enqueue_job("second").await;
    time::sleep(wait_time * 2).await;

    // Assert
    let results = fx.execution_updates.map_iter(|x| x.std_out).await;
    assert_eq!(results, ["one", "two"]);
}

// flakey on macOS, once i fix flakyness i can remove it
#[cfg_attr(target_os = "macos", test_with::no_env(GITHUB_ACTIONS))]
#[tokio::test]
async fn can_start_parallel_jobs_in_parallel() {
    // Arrange
    let fx = JobAgentFixture::new().await;
    let deps = Deps::new(Host, fx.settings.clone());

    let wait_time = Duration::from_millis(100);

    task::spawn(
        JobHandler::builder()
            .parallel("first", move |ctx| async move {
                time::sleep(wait_time).await;
                Ok(ctx.success().stdout("one"))
            })
            .parallel("second", async |ctx| Ok(ctx.success().stdout("two")))
            .build(deps)
            .run(),
    );

    // Act
    fx.enqueue_job("first").await;
    fx.enqueue_job("second").await;
    time::sleep(wait_time * 2).await;

    // Assert
    let results = fx.execution_updates.map_iter(|x| x.std_out).await;
    assert_eq!(results, ["two", "one"]);
}

#[tokio::test]
async fn parallel_jobs_dont_exceed_max() {
    // TODO!
}
