use orb_slot_ctrl::{
    program::{self, Cli},
    OrbSlotCtrl,
};

use orb_info::orb_os_release::OrbOsRelease;

use clap::Parser;
use color_eyre::eyre::Result;

fn main() -> Result<()> {
    let cli = Cli::parse();

    let orb_type = OrbOsRelease::read_blocking()?.orb_os_platform_type;

    let orb_slot_ctrl = OrbSlotCtrl::new("/", orb_type)?;

    program::run(&orb_slot_ctrl, cli)
}
