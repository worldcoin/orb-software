use std::path::{Path, PathBuf};

use clap::{
    builder::{styling::AnsiColor, Styles},
    Parser,
};
use color_eyre::{
    eyre::{ensure, OptionExt as _, WrapErr as _},
    Result, Section, SectionExt as _,
};
use orb_build_info::{make_build_info, BuildInfo};
use tracing::{info, level_filters::LevelFilter};
use tracing_subscriber::{
    fmt, layer::SubscriberExt as _, util::SubscriberInitExt as _, EnvFilter,
};

const EXPECTED_ZBUS_XMLGEN_VERSION: &str = "4.1.0";
const BUILD_INFO: BuildInfo = make_build_info!();

#[derive(Debug, clap::Parser)]
#[clap(
    version = BUILD_INFO.version,
    about,
    styles = clap_v3_styles(),
)]
struct Args {}

fn clap_v3_styles() -> Styles {
    Styles::styled()
        .header(AnsiColor::Yellow.on_default())
        .usage(AnsiColor::Green.on_default())
        .literal(AnsiColor::Green.on_default())
        .placeholder(AnsiColor::Green.on_default())
}

fn find_xml_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let file_name = entry.file_name().to_string_lossy().into_owned();
        if !file_name.ends_with(".xml") {
            continue;
        }
        files.push(entry.path());
    }

    Ok(files)
}

fn check_zbus_xmlgen_version() -> Result<()> {
    let output = std::process::Command::new("zbus-xmlgen")
        .arg("--version")
        .output()
        .wrap_err("failed to run `zbus-xmlgen --version`")
        .with_suggestion(|| "is zbus-xmlgen installed?")?;
    ensure!(
        output.status.success(),
        "nonzero exit code for `zbus-xmlgen --version`"
    );
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let stripped = stdout
        .strip_prefix("zbus-xmlgen ")
        .or_else(|| stdout.strip_prefix("zbus_xmlgen "))
        .unwrap_or(&stdout);

    (stripped == EXPECTED_ZBUS_XMLGEN_VERSION)
        .then_some(())
        .ok_or_eyre("unexpected command output")
        .section(stdout.clone().header("stdout:"))
}

fn generate_code_from_xml(xml_file: &Path, output_path: &Path) -> Result<()> {
    let exit_status = std::process::Command::new("zbus-xmlgen")
        .arg("file")
        .arg(xml_file.as_os_str())
        .arg("--output")
        .arg(output_path)
        .status()
        .wrap_err_with(|| {
            format!("failed to run `zbus-xmlgen file {}`", xml_file.display())
        })?;
    ensure!(exit_status.success(), "command failed");

    Ok(())
}

fn main() -> Result<()> {
    color_eyre::install()?;
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .init();

    let _args = Args::parse();

    let xml_folder = dbg!(Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("xml"));

    let generated_folder = dbg!(Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("src")
        .join("generated"));

    let tweaked_folder = dbg!(Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("src")
        .join("tweaked"));

    info!(
        "Listing contents of xml folder located at {}...",
        xml_folder.display()
    );
    ensure!(xml_folder.exists());

    let files = find_xml_files(&xml_folder)
        .wrap_err("error while searching through xml directory")
        .with_note(|| format!("xml directory was {}", xml_folder.display()))?;

    info!("{files:#?}");

    check_zbus_xmlgen_version()?;
    files
        .iter()
        .map(|xml_file_path| {
            let rust_file_name = format!(
                "{}.rs",
                xml_file_path.file_stem().unwrap().to_string_lossy()
            );
            (xml_file_path, rust_file_name)
        })
        // skip files that exist under `tweaked_folder`
        .filter(|(_, rust_file_name)| !tweaked_folder.join(rust_file_name).exists())
        .try_for_each(|(xml_file_path, rust_file_name)| {
            let out_file = generated_folder.join(rust_file_name);
            info!("Generating {out_file:?} from {xml_file_path:?}");
            generate_code_from_xml(xml_file_path, &out_file)
        })
        .wrap_err("error while generating code")?;

    Ok(())
}
