use orb_slot_ctrl::{
    program::{self, Cli},
    OrbSlotCtrl,
};

use clap::Parser;
use color_eyre::eyre::Result;
use efivar::EfiVarDb;

fn main() -> Result<()> {
    let cli = Cli::parse();

    let db = EfiVarDb::from_rootfs("/")?;
    let orb_slot_ctrl = OrbSlotCtrl::new(&db)?;

    program::run(&orb_slot_ctrl, cli)
}
