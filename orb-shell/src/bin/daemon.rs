use color_eyre::Result;
use orb_shell::shelld::ShellDaemon;
use speare::*;
use std::future;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    orb_telemetry::TelemetryConfig::new()
        .with_journald("orb-shelld")
        .init();
    
    let mut node = Node::default();
    node.spawn::<ShellDaemon>(());

    let _: () = future::pending().await;

    Ok(())
}
