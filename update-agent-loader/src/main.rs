use clap::Parser;
use eyre::{eyre, Result};
use tracing_subscriber::EnvFilter;
use url::Url;

/// Update agent loader that downloads and executes a binary from a URL
#[derive(Parser, Debug)]
#[clap(author, version, about, trailing_var_arg = true)]
struct Args {
    /// URL to download the executable from
    #[clap(short, long)]
    url: String,

    /// Arguments to pass to the downloaded executable
    /// All arguments after -- will be passed to the executable
    #[clap(last = true)]
    exec_args: Vec<String>,
}

fn main() -> Result<()> {
    // Initialize tracing with env_logger compatibility
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    // Parse command line arguments
    let args = Args::parse();

    let url = Url::parse(&args.url)?;

    // Get arguments after -- to pass to the executable
    let exec_args: Vec<&str> = args.exec_args.iter().map(|s| s.as_str()).collect();

    // Download and execute in one step
    match update_agent_loader::download_and_execute(&url, &exec_args) {
        Ok(_) => unreachable!("fexecve succeeded - this process has been replaced"),
        Err(e) => Err(eyre!("Failed to download or execute from {}: {}", url, e)),
    }
}
