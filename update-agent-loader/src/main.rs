use clap::Parser;
use eyre::{eyre, Result};
use url::Url;

/// Update agent loader that downloads and executes a binary from a URL
#[derive(Parser, Debug)]
#[clap(author, version, about)]
struct Args {
    /// URL to download the executable from
    #[clap(short, long)]
    url: Option<String>,

    /// Arguments to pass to the executable
    #[clap(short, long, value_delimiter = ' ', num_args = 0..)]
    args: Vec<String>,
}

fn main() -> Result<()> {
    // Parse command line arguments
    let args = Args::parse();

    // Use provided URL or fallback to default
    let url_str = args
        .url
        .unwrap_or_else(|| "https://example.com/path/to/executable".to_string());
    let url = Url::parse(&url_str)?;

    // Use provided arguments or empty vector
    let exec_args: Vec<&str> = args.args.iter().map(|s| s.as_str()).collect();

    // Download and execute in one step
    match update_agent_loader::download_and_execute(&url, &exec_args) {
        Ok(_) => unreachable!("fexecve succeeded - this process has been replaced"),
        Err(e) => Err(eyre!("Failed to download or execute: {}", e)),
    }
}
