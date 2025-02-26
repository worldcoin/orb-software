mod video;

use clap::Parser;
use color_eyre::{eyre::WrapErr as _, Result};
use futures::FutureExt as _;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info};

const CONTROL_QUEUE_SIZE: usize = 8;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();

    let cancel = CancellationToken::new();
    let cancel_clone = cancel.clone();
    tokio::task::spawn(async move {
        let _cancel_guard = cancel_clone.drop_guard();
        tokio::signal::ctrl_c().await.expect("failed to get ctrlc");
        info!("got ctrlc, signalling tasks to shutdown");
    });
    let flusher = orb_telemetry::TelemetryConfig::new().init();
    info!("starting server");
    let result = run(args, cancel).await;

    flusher.flush().await;
    result
}

#[derive(Debug, Parser, Clone)]
struct Args {
    /// The port to use for the http server
    #[clap(long, default_value = "8443")]
    http_port: u16,
    /// The port to use for the webtransport server
    #[clap(long, default_value = "1337")]
    wt_port: u16,
}

async fn run(args: Args, cancel: CancellationToken) -> Result<()> {
    let _cancel_guard = cancel.clone().drop_guard();

    let video_cancel = cancel.child_token();
    let video_task_handle = crate::video::VideoTaskHandle::spawn(video_cancel);

    let wt_cancel = cancel.child_token();
    let (control_tx, control_rx) = mpsc::channel(CONTROL_QUEUE_SIZE);
    let wt_cfg = orb_wt_video::wt_server::Config::builder()
        .identity_self_signed(["localhost", "127.0.0.1", "::1"])
        .cancel(wt_cancel.clone())
        .png_rx(video_task_handle.frame_rx)
        .port(args.wt_port)
        .control_tx(control_tx)
        .build();
    let wt_identity = wt_cfg.identity.clone_identity();
    let wt_server = wt_cfg
        .bind()
        .wrap_err("webtransport server failed to bind")?;
    info!(
        "webtransport server running on address {}",
        wt_server.local_addr()
    );
    let wt_task = tokio::task::spawn(wt_server.run());

    let http_cancel = cancel.child_token();
    let http_task = orb_wt_video::http_server::Config::builder()
        .wt_identity(wt_identity)
        .tls_config_from_wt_identity()
        .await
        .port(args.http_port)
        .cancel(http_cancel)
        .build()
        .spawn()
        .await
        .wrap_err("failed to spawn http task")?;
    info!("http server running on address {}", http_task.local_addr);

    let listen_cancel = cancel.child_token();
    let listen_task = tokio::task::spawn(async move {
        listen_cancel
            .run_until_cancelled(listen_to_commands(control_rx))
            .await;
        debug!("cancelling listen task");
    });

    let ((), (), (), ()) = tokio::try_join!(
        video_task_handle.task_handle.map(|r| {
            debug!("video task finished");
            r.wrap_err("video task panicked")
        }),
        wt_task.map(|r| {
            debug!("wt task finished");
            r.wrap_err("wt task panicked")?
                .wrap_err("wt task returned error")
        }),
        http_task.task_handle.map(|r| {
            debug!("http task finished");
            r.wrap_err("http task panicked")?
                .wrap_err("http task returned error")
        }),
        listen_task.map(|r| {
            debug!("listen task finished");
            r.wrap_err("listen task panicked")
        }),
    )?;

    Ok(())
}

async fn listen_to_commands(
    mut control_rx: mpsc::Receiver<orb_wt_video::control::ControlEvent>,
) {
    while let Some(control) = control_rx.recv().await {
        info!(?control, "got control event");
    }
}
