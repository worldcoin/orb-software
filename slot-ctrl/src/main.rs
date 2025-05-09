use orb_slot_ctrl::{
    program::{self, Cli},
    OrbSlotCtrl,
};

use clap::Parser;
use color_eyre::eyre::Result;

fn main() -> Result<()> {
    let cli = Cli::parse();

    let orb_slot_ctrl = OrbSlotCtrl::new("/")?;

    program::run(&orb_slot_ctrl, cli)
}
