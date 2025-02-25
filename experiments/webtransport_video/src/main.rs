mod video;

use clap::Parser;
use color_eyre::{eyre::WrapErr as _, Result};
use tokio_util::sync::CancellationToken;
use tracing::info;
use wtransport::tls::Sha256DigestFmt;

use orb_wt_video::networking::{run_http_server, run_wt_server};

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();

    let cancel = CancellationToken::new();
    let cancel_clone = cancel.clone();
    tokio::task::spawn(async move {
        let _cancel_guard = cancel_clone.drop_guard();
        tokio::signal::ctrl_c().await.expect("failed to get ctrlc");
    });
    let flusher = orb_telemetry::TelemetryConfig::new().init();
    info!("starting server");
    let result = run(args, cancel).await;

    flusher.flush().await;
    result
}

async fn run(args: Args, cancel: CancellationToken) -> Result<()> {
    let _cancel_guard = cancel.clone().drop_guard();

    let identity = wtransport::Identity::self_signed_builder()
        .subject_alt_names(["localhost", "127.0.0.1", "::1"])
        .from_now_utc()
        .validity_days(7)
        .build()
        .unwrap();

    let server_certificate_hashes = identity.certificate_chain().as_slice()[0]
        .hash()
        .fmt(Sha256DigestFmt::BytesArray);
    info!("server certificate hashes: {}", server_certificate_hashes);

    let video_task = crate::video::VideoTaskHandle::spawn(cancel.child_token());

    let wt_fut = async {
        let cancel = cancel.child_token();
        cancel
            .run_until_cancelled(run_wt_server(
                args.clone(),
                cancel.clone(),
                identity.clone_identity(),
                video_task.frame_rx,
            ))
            .await
            .unwrap_or(Ok(()))
    };

    let http_fut = async {
        let cancel = cancel.child_token();
        cancel
            .run_until_cancelled(run_http_server(
                args.clone(),
                cancel.clone(),
                identity.clone_identity(),
            ))
            .await
            .unwrap_or(Ok(()))
    };

    let video_task_fut =
        async { video_task.task_handle.await.wrap_err("video task panicked") };
    let ((), (), ()) = tokio::try_join!(wt_fut, http_fut, video_task_fut)?;
    Ok(())
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
