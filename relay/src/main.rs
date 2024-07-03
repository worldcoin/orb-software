use clap::{
    builder::{styling::AnsiColor, Styles},
    Parser,
};
use color_eyre::{eyre::WrapErr as _, Result};
use futures::FutureExt as _;
use orb_build_info::{make_build_info, BuildInfo};
use orb_relay_messages::{
    relay::{OrbCommand, OrbEvent},
    tonic,
};
use tokio::sync::mpsc;

type OrbServiceClient = orb_relay_messages::relay::orb_service_client::OrbServiceClient<
    tonic::transport::Channel,
>;

static BUILD_INFO: BuildInfo = make_build_info!();

/// Utility args
#[derive(Parser, Debug)]
#[clap(
    about,
    styles = clap_v3_styles(),
    version = BUILD_INFO.version,
)]
struct Args {
    #[clap(short, long)]
    relay_url: Option<String>,
}

fn clap_v3_styles() -> Styles {
    Styles::styled()
        .header(AnsiColor::Yellow.on_default())
        .usage(AnsiColor::Green.on_default())
        .literal(AnsiColor::Green.on_default())
        .placeholder(AnsiColor::Green.on_default())
}

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    tracing_subscriber::fmt::init();
    let args = Args::parse();

    let mut client =
        orb_relay_messages::relay::orb_service_client::OrbServiceClient::connect(
            args.relay_url
                .unwrap_or_else(|| "http://[::1]:50051".to_owned()),
        )
        .await?;

    println!("connected");
    let (task, event_tx, command_rx) = spawn_grpc(client);

    println!("created stream");

    let _: ((),) = tokio::try_join!(
        task.map(|r| r.wrap_err("notify_state task exited unexpectedly")?),
    )?;

    Ok(())
}

fn spawn_grpc(
    mut client: OrbServiceClient,
) -> (
    tokio::task::JoinHandle<Result<()>>,
    mpsc::Sender<OrbEvent>,
    mpsc::Receiver<OrbCommand>,
) {
    let (event_tx, event_rx) = tokio::sync::mpsc::channel(1);
    let mut event_rx = tokio_stream::wrappers::ReceiverStream::new(event_rx);
    let (command_tx, command_rx) = tokio::sync::mpsc::channel(1);

    let task: tokio::task::JoinHandle<Result<()>> = tokio::spawn(async move {
        let command_rx = client
            .orb_connect(&mut event_rx)
            .await
            .wrap_err("failed to establish bidirectional stream via `orb_connect`")?;
        //
        std::future::pending().await
    });
    (task, event_tx, command_rx)
}
