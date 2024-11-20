mod conversions;

use std::io::{Read as _, Write as _};

use clap::{
    builder::{styling::AnsiColor, Styles},
    command, Parser,
};
use color_eyre::eyre::{ensure, WrapErr as _};
use ed25519_dalek::pkcs8::DecodePrivateKey as _;

use crate::conversions::{dalek_signing_key_to_jwk, ExportPrivKeys};

const BUILD_INFO: orb_build_info::BuildInfo = orb_build_info::make_build_info!();

fn clap_v3_styles() -> Styles {
    Styles::styled()
        .header(AnsiColor::Yellow.on_default())
        .usage(AnsiColor::Green.on_default())
        .literal(AnsiColor::Green.on_default())
        .placeholder(AnsiColor::Green.on_default())
}

#[derive(Debug, Parser)]
#[command(
    author,
    version = BUILD_INFO.version,
    styles = clap_v3_styles(),
)]
struct Args {
    /// The input format
    #[clap(long)]
    in_fmt: Format,
    /// The output format
    #[clap(long)]
    out_fmt: Format,
    /// The file to read as input.
    #[clap(long)]
    in_file: std::path::PathBuf,
    /// The file to write as output.
    #[clap(long)]
    out_file: Option<std::path::PathBuf>,
    /// If provided, the output format will include private keys. Will error if the
    /// input format doesn't have any private keys
    #[clap(long)]
    export_priv_keys: bool,
}

#[derive(Debug, Eq, PartialEq, clap::ValueEnum, Clone, Copy)]
enum Format {
    Pkcs8,
    Jwk,
}

fn main() -> color_eyre::Result<()> {
    let args = Args::parse();

    ensure!(
        args.in_fmt == Format::Pkcs8,
        "todo: input formats other than PKCS8 are not yet supported"
    );

    let mut pem_contents = String::new();
    std::fs::File::open(args.in_file)
        .wrap_err("failed to open in_file")?
        .read_to_string(&mut pem_contents)
        .wrap_err("error while reading file")?;

    // TODO: We should also support pem public keys
    let signing_key = ed25519_dalek::SigningKey::from_pkcs8_pem(&pem_contents)
        .wrap_err("failed to parse PEM contents into ed25519 signing key")?;

    let jwk = dalek_signing_key_to_jwk(
        &signing_key,
        if args.export_priv_keys {
            ExportPrivKeys::True
        } else {
            ExportPrivKeys::False
        },
    );
    let jwk_string =
        serde_json::to_string(&jwk).wrap_err("failed to serialize jwk to string")?;

    if let Some(out_path) = args.out_file {
        let mut out_file =
            std::fs::File::create(out_path).wrap_err("failed to create output file")?;
        out_file
            .write_all(jwk_string.as_bytes())
            .wrap_err("failed to write to file")?;
    } else {
        println!("{jwk_string}");
    }

    Ok(())
}
