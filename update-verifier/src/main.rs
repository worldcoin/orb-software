mod telemetry;

use crate::telemetry::ExecContext;
use clap::{
    builder::{styling::AnsiColor, Styles},
    Parser,
};
use eyre::{self, Context};
use orb_slot_ctrl::{EfiVarDb, OrbSlotCtrl};
use orb_update_verifier::BUILD_INFO;
use tracing::{error, metadata::LevelFilter};

#[derive(Parser, Debug)]
#[clap(
    version = BUILD_INFO.version,
    about,
    styles = clap_v3_styles(),
)]
struct Cli {}

fn clap_v3_styles() -> Styles {
    Styles::styled()
        .header(AnsiColor::Yellow.on_default())
        .usage(AnsiColor::Green.on_default())
        .literal(AnsiColor::Green.on_default())
        .placeholder(AnsiColor::Green.on_default())
}

fn main() -> eyre::Result<()> {
    run().inspect_err(|error| error!(?error, "failed to run update-verifier"))
}

fn run() -> eyre::Result<()> {
    telemetry::start::<ExecContext, _>(LevelFilter::INFO, std::io::stdout)
        .wrap_err("update verifier encountered error while starting telemetry")?;

    let _args = Cli::parse();

    let efi_var_db = EfiVarDb::from_rootfs("/")?;
    let orb_slot_ctrl = OrbSlotCtrl::new(&efi_var_db)?;
    orb_update_verifier::run_health_check(orb_slot_ctrl)
        .wrap_err("update verifier encountered error while checking system health")?;

    Ok(())
}
