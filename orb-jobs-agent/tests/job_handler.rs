use color_eyre::eyre::bail;
use common::{fake_connd::MockConnd, fixture::JobAgentFixture};
use orb_jobs_agent::{
    job_system::{ctx::JobExecutionUpdateExt, handler::JobHandler},
    program::Deps,
    shell::Host,
};
use orb_relay_messages::jobs::v1::JobExecutionStatus;
use std::time::Duration;
use tokio::{
    task,
    time::{self, Instant},
};

mod common;

#[tokio::test]
async fn sequential_jobs_block_other_jobs_execution() {
    // Arrange
    let fx = JobAgentFixture::new().await;
    let deps = Deps::new(Host, fx.dbus_conn.clone(), fx.settings.clone());

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
    fx.enqueue_job("second").await.wait_for_completion().await;

    // Assert
    let results = fx.execution_updates.map_iter(|x| x.std_out).await;
    assert_eq!(results, ["one", "two"]);
}

#[tokio::test]
async fn can_start_parallel_jobs_in_parallel() {
    // Arrange
    let fx = JobAgentFixture::new().await;
    let deps = Deps::new(Host, fx.dbus_conn.clone(), fx.settings.clone());

    let wait_time = Duration::from_millis(500);

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
    let ticket = fx.enqueue_job("first").await;
    fx.enqueue_job("second").await;
    ticket.wait_for_completion().await;

    // Assert
    let results = fx.execution_updates.map_iter(|x| x.std_out).await;
    assert_eq!(results, ["two", "one"]);
}

#[tokio::test]
async fn parallel_jobs_dont_exceed_max() {
    // TODO!
}

#[tokio::test]
async fn gracefully_handles_unsupported_cmds() {
    // Arrange
    let fx = JobAgentFixture::new().await;
    let deps = Deps::new(Host, fx.dbus_conn.clone(), fx.settings.clone());

    task::spawn(JobHandler::builder().build(deps).run());

    // Act
    fx.enqueue_job("joberoni").await.wait_for_completion().await;

    // Assert
    let results = fx.execution_updates.map_iter(|x| x.status).await;
    assert_eq!(results, [JobExecutionStatus::FailedUnsupported as i32]);
}

#[tokio::test]
async fn it_cancels_a_long_running_job() {
    // Arrange
    let fx = JobAgentFixture::with_namespace("cancel_long_running_job").await;
    let deps = Deps::new(Host, fx.dbus_conn.clone(), fx.settings.clone());

    let wait_time = Duration::from_millis(50);

    task::spawn(
        JobHandler::builder()
            .parallel("timeout", move |ctx| async move {
                let start = Instant::now();

                loop {
                    if start.elapsed() > Duration::from_secs(5) {
                        bail!("timed out!");
                    }

                    if ctx.is_cancelled() {
                        break;
                    }

                    println!("looping!");
                    time::sleep(wait_time).await;
                }

                Ok(ctx.success().stdout("cancelled succesfully!"))
            })
            .build(deps)
            .run(),
    );

    // Act
    let ticket = fx.enqueue_job("timeout").await;
    time::sleep(wait_time * 4).await;
    fx.cancel_job(&ticket.exec_id).await;
    ticket.wait_for_completion().await;

    // Assert
    let results = fx.execution_updates.map_iter(|x| x.std_out).await;
    assert_eq!(results[0], "cancelled succesfully!");
}
