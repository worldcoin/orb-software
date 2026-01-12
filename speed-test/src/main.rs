use clap::Parser;
use color_eyre::eyre::Result;
use orb_build_info::{make_build_info, BuildInfo};
use orb_speed_test::run_speed_test;

const BUILD_INFO: BuildInfo = make_build_info!();

#[derive(Parser, Debug)]
#[command(name = "orb-speed-test")]
#[command(version = BUILD_INFO.version)]
#[command(about = "Network speed test utility for Orb")]
struct Args {
    /// Output format
    #[arg(long, default_value = "human", value_parser = ["json", "human"])]
    format: String,

    /// Test size in megabytes (MB)
    #[arg(long, default_value_t = 30)]
    size: usize,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();
    run(&args).await
}

async fn run(args: &Args) -> Result<()> {
    println!("Starting speed test with test size {} Mb", args.size);

    let size_bytes = args.size * 1_000_000;
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

    Ok(())
}
