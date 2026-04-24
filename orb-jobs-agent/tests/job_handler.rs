use color_eyre::eyre::bail;
use common::fixture::JobAgentFixture;
use orb_jobs_agent::{
    job_system::{ctx::JobExecutionUpdateExt, handler::JobHandler},
    shell::Host,
};
use orb_relay_messages::jobs::v1::JobExecutionStatus;
use std::time::Duration;
use tokio::{
    task,
    time::{self, Instant},
};
use zenorb::zoci::ZociQueryExt;

mod common;

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn sequential_jobs_block_other_jobs_execution() {
    // Arrange
    let fx = JobAgentFixture::new().await;
    let deps = fx.deps(Host);

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

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn can_start_parallel_jobs_in_parallel() {
    // Arrange
    let fx = JobAgentFixture::new().await;
    let deps = fx.deps(Host);

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

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn parallel_jobs_dont_exceed_max() {
    // TODO!
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn gracefully_handles_unsupported_cmds() {
    // Arrange
    let fx = JobAgentFixture::new().await;
    let deps = fx.deps(Host);

    task::spawn(JobHandler::builder().build(deps).run());

    // Act
    fx.enqueue_job("joberoni").await.wait_for_completion().await;

    // Assert
    let results = fx.execution_updates.map_iter(|x| x.status).await;
    assert_eq!(results, [JobExecutionStatus::FailedUnsupported as i32]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn routes_unknown_job_to_zoci_queryable_and_forwards_args() {
    // Arrange
    let fx = JobAgentFixture::with_namespace("zoci_fallback_stdout").await;
    let _program = fx.program().shell(Host).spawn().await;

    let zoci = fx.zenorb_service("echo").await;
    let queryable = zoci.declare_queryable("job/read_temp").await.unwrap();

    task::spawn(async move {
        let query = queryable.recv_async().await.unwrap();
        let args: (String, String) = query.args().unwrap();
        query.res(&args).await.unwrap();
    });

    time::sleep(Duration::from_millis(300)).await;

    // Act
    fx.enqueue_job("read_temp sensor-a nominal")
        .await
        .wait_for_completion()
        .await;

    // Assert
    let result = fx.execution_updates.read().await;

    assert_eq!(result[0].status, JobExecutionStatus::Succeeded as i32);
    assert_eq!(result[0].std_out, "[\"sensor-a\",\"nominal\"]");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn failed_unsupported_job_does_not_break_later_zoci_handler() {
    // Arrange
    let fx = JobAgentFixture::with_namespace("zoci_fallback_recovery").await;
    let _program = fx.program().shell(Host).spawn().await;

    // Act
    fx.enqueue_job("read_temp sensor-a")
        .await
        .wait_for_completion()
        .await;

    let zoci = fx.zenorb_service("echo").await;
    let queryable = zoci.declare_queryable("job/read_temp").await.unwrap();

    task::spawn(async move {
        let query = queryable.recv_async().await.unwrap();
        let payload = query.payload().unwrap();
        let arg = payload.try_to_string().unwrap();

        query.res(arg.as_ref()).await.unwrap();
    });

    time::sleep(Duration::from_millis(300)).await;

    fx.enqueue_job("read_temp sensor-a")
        .await
        .wait_for_completion()
        .await;

    // Assert
    let result = fx.execution_updates.read().await;

    assert_eq!(
        result[0].status,
        JobExecutionStatus::FailedUnsupported as i32
    );
    assert_eq!(result[1].status, JobExecutionStatus::Succeeded as i32);
    assert_eq!(result[1].std_out, "\"sensor-a\"");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn routes_zoci_reply_err_to_failed_job_with_stderr() {
    // Arrange
    let fx = JobAgentFixture::with_namespace("zoci_fallback_stderr").await;
    let _program = fx.program().shell(Host).spawn().await;

    let zoci = fx.zenorb_service("echo").await;
    let queryable = zoci.declare_queryable("job/read_temp").await.unwrap();

    task::spawn(async move {
        let query = queryable.recv_async().await.unwrap();
        let args: (String, String) = query.args().unwrap();
        query.res_err(&args).await.unwrap();
    });

    time::sleep(Duration::from_millis(300)).await;

    // Act
    fx.enqueue_job("read_temp sensor-a nominal")
        .await
        .wait_for_completion()
        .await;

    // Assert
    let result = fx.execution_updates.read().await;

    assert_eq!(result[0].status, JobExecutionStatus::Failed as i32);
    assert_eq!(result[0].std_err, "[\"sensor-a\",\"nominal\"]");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn registered_handler_takes_precedence_over_zoci_fallback() {
    // Arrange
    let fx = JobAgentFixture::with_namespace("zoci_fallback_precedence").await;
    let deps = fx.deps(Host);

    task::spawn(
        JobHandler::builder()
            .parallel("read_temp", async |ctx| {
                Ok(ctx.success().stdout("local handler won"))
            })
            .build(deps)
            .run(),
    );

    let zoci = fx.zenorb_service("echo").await;
    let queryable = zoci.declare_queryable("job/read_temp").await.unwrap();
    let (query_seen_tx, mut query_seen_rx) = tokio::sync::oneshot::channel();

    let recv_task = task::spawn(async move {
        if let Ok(_query) = queryable.recv_async().await {
            let _ = query_seen_tx.send(());
        }
    });

    time::sleep(Duration::from_millis(300)).await;

    // Act
    fx.enqueue_job("read_temp sensor-a nominal")
        .await
        .wait_for_completion()
        .await;

    // Assert
    let result = fx.execution_updates.read().await;

    assert_eq!(result[0].status, JobExecutionStatus::Succeeded as i32);
    assert_eq!(result[0].std_out, "local handler won");
    assert!(query_seen_rx.try_recv().is_err());

    recv_task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn it_cancels_a_long_running_job() {
    // Arrange
    let fx = JobAgentFixture::with_namespace("cancel_long_running_job").await;
    let deps = fx.deps(Host);

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
