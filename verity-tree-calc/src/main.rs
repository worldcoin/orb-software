#![forbid(unsafe_code)]

use std::io::{self, Write};

use clap::Parser;
use orb_build_info::{make_build_info, BuildInfo};
use tracing::debug;

// TODO @oldgalileo document the math and magic consts

const BUILD_INFO: BuildInfo = make_build_info!();

#[derive(Parser, Debug)]
#[command(about, author, version=BUILD_INFO.version, styles=make_clap_v3_styles())]
struct Args {
    #[clap(value_parser)]
    data_size: u64,
}

fn main() {
    tracing_subscriber::FmtSubscriber::builder()
        .with_writer(io::stderr)
        .init();

    let args = Args::parse();
    debug!(args = ?args, "parsed arguments");

    let data_blocks = ((args.data_size + 4095) & !4095) / 4096;
    let hash_block_size = 4096;
    let digest_size = 32;
    let hash_per_block_bits: u32 = ((hash_block_size / digest_size) as u64).ilog2();

    let mut levels = 0;
    while hash_per_block_bits * levels < 64
        && ((data_blocks - 1) >> (hash_per_block_bits * levels)) != 0
    {
        levels += 1;
    }

    let mut hash_position = 0;
    for i in (0..levels).rev() {
        let s_shift = (i + 1) * hash_per_block_bits;
        assert!(s_shift <= 63, "should not overflow");

        let s =
            (data_blocks + (1u64 << s_shift) - 1) >> ((i + 1) * hash_per_block_bits);
        hash_position += s;
    }

    let hash_position = ((hash_position + 4096) + 4095) & !4095;

    // let hash_position = ((hash_position + 4096 - 1) / 4096) + 1;
    // let hash_position = hash_position * 4096;

    io::stdout()
        .write_all(
            format!(
                "{} {}",
                (data_blocks * 4096) - args.data_size,
                hash_position
            )
            .as_bytes(),
        )
        .unwrap();
}

fn make_clap_v3_styles() -> clap::builder::Styles {
    use clap::builder::styling::AnsiColor;
    clap::builder::Styles::styled()
        .header(AnsiColor::Yellow.on_default())
        .usage(AnsiColor::Green.on_default())
        .literal(AnsiColor::Green.on_default())
        .placeholder(AnsiColor::Green.on_default())
}

#[cfg(test)]
mod test {
    // TODO @oldgalileo write some tests pls to document how this is supposed
    // to be used 👉👈
}
