#![forbid(unsafe_code)]
use color_eyre::{eyre::WrapErr as _, Result};
use futures::FutureExt;
use orb_mcu_interface::{can::canfd::CanRawMessaging, Device};
use tracing::level_filters::LevelFilter;
use tracing_subscriber::{
    layer::SubscriberExt as _, util::SubscriberInitExt as _, EnvFilter,
};

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .init();

    let (msg_tx, msg_rx) = std::sync::mpsc::channel();
    let _iface = CanRawMessaging::new(String::from("can0"), Device::Security, msg_tx)
        .wrap_err("failed to create messaging interface")?;

    let msg_task = tokio::task::spawn_blocking(move || {
        while let Ok(msg) = msg_rx.recv() {
            println!("{msg:?}");
        }
    })
    .map(|err| err.wrap_err("message task terminated unexpectedly"));

    tokio::select! {
        result = msg_task => result,
        result = tokio::signal::ctrl_c() => { println!("ctrl-c detected"); result.wrap_err("failed to listen for ctrl-c")}

    }
}
