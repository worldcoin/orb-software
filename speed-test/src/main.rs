use clap::Parser;
use color_eyre::eyre::Result;
use orb_build_info::{make_build_info, BuildInfo};
use orb_info::orb_id::OrbId;
use orb_speed_test::{run_pcp_speed_test, run_speed_test};
use uuid::Uuid;

const BUILD_INFO: BuildInfo = make_build_info!();

#[derive(Parser, Debug)]
#[command(name = "orb-speed-test")]
#[command(version = BUILD_INFO.version)]
#[command(about = "Network speed test utility for Orb")]
struct Args {
    /// Output format
    #[arg(long, default_value = "human", value_parser = ["json", "human"])]
    format: String,

    /// Test size in megabytes (MB) of uncompressed data
    #[arg(long)]
    size: Option<usize>,

    /// Run PCP upload speed test instead of Cloudflare test
    #[arg(long)]
    pcp: bool,

    /// D-Bus socket address for PCP authentication (only used with --pcp)
    #[arg(long, default_value = "unix:path=/tmp/worldcoin_bus_socket")]
    dbus_addr: String,

    /// Number of uploads to perform for averaging (only used with --pcp)
    #[arg(long, default_value = "3")]
    num_uploads: usize,
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();
    run(&args).await
}

async fn run(args: &Args) -> Result<()> {
    if args.pcp {
        let size_mb = args.size.unwrap_or(20);
        let size_bytes = size_mb * 1_000_000;
        let orb_id = OrbId::read().await?;
        let session_id = Uuid::new_v4().to_string();

        println!("Starting PCP upload speed: {} Mb (uncompressed)", size_mb);

        let results = run_pcp_speed_test(
            size_bytes,
            &orb_id,
            &session_id,
            &args.dbus_addr,
            args.num_uploads,
        )
        .await?;

        match args.format.as_str() {
            "json" => {
                let json = serde_json::to_string_pretty(&results)?;
                println!("{}", json);
            }
            "human" => {
                println!("Connectivity Quality (PCP): {:?}", results.connectivity);
                println!(
                    "PCP Upload (avg): {:.2} Mbps ({:.1} Mb in {} ms)",
                    results.upload_mbps, results.upload_mb, results.upload_duration_ms
                );
            }
            _ => unreachable!("Invalid format validated by clap"),
        }
    } else {
        let size_mb = args.size.unwrap_or(30);
        let size_bytes = size_mb * 1_000_000;

        println!("Starting speed test: {} Mb", size_mb);

        let results = run_speed_test(size_bytes).await?;

        match args.format.as_str() {
            "json" => {
                let json = serde_json::to_string_pretty(&results)?;
                println!("{}", json);
            }
            "human" => {
                println!("Connectivity Quality: {:?}", results.connectivity);
                println!(
                    "Upload:   {:.2} Mbps ({:.1} Mb in {} ms)",
                    results.upload_mbps, results.upload_mb, results.upload_duration_ms
                );
                println!(
                    "Download: {:.2} Mbps ({:.1} Mb in {} ms)",
                    results.download_mbps,
                    results.download_mb,
                    results.download_duration_ms
                );
            }
            _ => unreachable!("Invalid format validated by clap"),
        }
    }

    Ok(())
}
