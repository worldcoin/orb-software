use clap::Parser;
use orb_slot_ctrl::{
    program::{self, Cli},
    EfiVarDb, OrbSlotCtrl,
};

fn main() -> eyre::Result<()> {
    let cli = Cli::parse();

    let db = EfiVarDb::from_rootfs("/")?;
    let orb_slot_ctrl = OrbSlotCtrl::new(&db)?;

    program::run(&orb_slot_ctrl, cli)
}
