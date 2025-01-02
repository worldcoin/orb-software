use color_eyre::Result;
use orb_shell::shellc::ShellClient;
use speare::Node;
use std::future;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    orb_telemetry::TelemetryConfig::new().init();

    let mut node = Node::default();
    node.spawn::<ShellClient>(());

    let _: () = future::pending().await;

    Ok(())
}

// TODO: receive "port"  (orbid) as argument
// TODO: orb-attest token
// graceful exit
// test keepalive
