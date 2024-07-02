use clap::{
    builder::{styling::AnsiColor, Styles},
    Parser,
};
use orb_build_info::{make_build_info, BuildInfo};

static BUILD_INFO: BuildInfo = make_build_info!();

/// Utility args
#[derive(Parser, Debug)]
#[clap(
    author,
    version = BUILD_INFO.version,
    about,
    styles = clap_v3_styles(),
)]
struct Args {
    relay_url: String,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
}

fn clap_v3_styles() -> Styles {
    Styles::styled()
        .header(AnsiColor::Yellow.on_default())
        .usage(AnsiColor::Green.on_default())
        .literal(AnsiColor::Green.on_default())
        .placeholder(AnsiColor::Green.on_default())
}
