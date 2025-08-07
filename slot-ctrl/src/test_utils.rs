use std::fs;

use crate::{
    program::{self, Cli},
    EfiVarDb, OrbSlotCtrl, RetryCount, RootFsStatus, Slot,
};
use bon::bon;
use clap::Parser;
use orb_info::orb_os_release::OrbOsPlatform;
use tempfile::TempDir;

/// A Fixture that initializes fake EfiVars.
/// Both Rootfs slots are normal by default.
pub struct Fixture {
    _tempdir: TempDir,
    pub db: EfiVarDb,
    pub slot_ctrl: OrbSlotCtrl,
}

#[bon]
impl Fixture {
    #[builder]
    pub fn new(
        #[builder(finish_fn)] orb: OrbOsPlatform,
        #[builder(default = Slot::A)] current_slot: Slot,
        #[builder(default = Slot::A)] next_slot: Slot,
        #[builder(default = RootFsStatus::Normal)] status_a: RootFsStatus,
        #[builder(default = RootFsStatus::Normal)] status_b: RootFsStatus,
        #[builder(default = 3)] retry_count_a: u8,
        #[builder(default = 3)] retry_count_b: u8,
        #[builder(default = 3)] retry_count_max: u8,
    ) -> Fixture {
        let tempdir = TempDir::new_in("/tmp").unwrap();
        let db_path = tempdir.path().join("sys/firmware/efi/efivars/");
        fs::create_dir_all(&db_path).unwrap();

        let db = EfiVarDb::from_rootfs(&tempdir).unwrap();
        let slot_ctrl = OrbSlotCtrl::from_efivar_db(&db, orb).unwrap();

        db.get_var(Slot::CURRENT_SLOT_PATH)
            .unwrap()
            .write(&current_slot.to_efivar_data())
            .unwrap();

        db.get_var(Slot::NEXT_SLOT_PATH)
            .unwrap()
            .write(&next_slot.to_efivar_data())
            .unwrap();

        db.get_var(RetryCount::COUNT_A_PATH)
            .unwrap()
            .write(&RetryCount(retry_count_a).to_efivar_data())
            .unwrap();

        db.get_var(RetryCount::COUNT_B_PATH)
            .unwrap()
            .write(&RetryCount(retry_count_b).to_efivar_data())
            .unwrap();

        db.get_var(RetryCount::COUNT_MAX_PATH)
            .unwrap()
            .write(&RetryCount(retry_count_max).to_efivar_data())
            .unwrap();

        db.get_var(RootFsStatus::STATUS_A_PATH)
            .unwrap()
            .write(&status_a.to_efivar_data(orb).unwrap())
            .unwrap();

        db.get_var(RootFsStatus::STATUS_B_PATH)
            .unwrap()
            .write(&status_b.to_efivar_data(orb).unwrap())
            .unwrap();

        Self {
            _tempdir: tempdir,
            db,
            slot_ctrl,
        }
    }

    pub fn run(&self, cmd: &str) -> color_eyre::Result<String> {
        let cmd: Vec<_> = cmd.split(" ").collect();
        let mut vec = Vec::from(&["slot-ctrl"]);
        vec.extend_from_slice(&cmd);
        println!("{vec:?}");

        let cli = Cli::try_parse_from(&vec)?;
        program::run(&self.slot_ctrl, cli)
    }
}
