use std::fs;

use crate::{EfiVarDb, OrbSlotCtrl, RetryCount, RootFsStatus, Slot};
use orb_info::orb_os_release::OrbType;
use tempfile::TempDir;

/// A Fixture that initializes fake EfiVars.
/// Both Rootfs slots are normal by default.
pub struct Fixture {
    _tempdir: TempDir,
    pub db: EfiVarDb,
    pub slot_ctrl: OrbSlotCtrl,
}

impl Fixture {
    pub fn new(orb: OrbType, current_and_next_slot: Slot, max_retry_count: u8) -> Self {
        let tempdir = TempDir::new_in("/tmp").unwrap();
        let db_path = tempdir.path().join("sys/firmware/efi/efivars/");
        fs::create_dir_all(&db_path).unwrap();

        let db = EfiVarDb::from_rootfs(&tempdir).unwrap();

        let orb_type = OrbType::Pearl;
        let slot_ctrl = OrbSlotCtrl::from_efivar_db(&db, orb_type).unwrap();

        db.get_var(Slot::CURRENT_SLOT_PATH)
            .unwrap()
            .write(&current_and_next_slot.to_efivar_data())
            .unwrap();

        db.get_var(Slot::NEXT_SLOT_PATH)
            .unwrap()
            .write(&current_and_next_slot.to_efivar_data())
            .unwrap();

        db.get_var(RetryCount::COUNT_A_PATH)
            .unwrap()
            .write(&RetryCount(0).to_efivar_data())
            .unwrap();

        db.get_var(RetryCount::COUNT_B_PATH)
            .unwrap()
            .write(&RetryCount(0).to_efivar_data())
            .unwrap();

        db.get_var(RetryCount::COUNT_MAX_PATH)
            .unwrap()
            .write(&RetryCount(max_retry_count).to_efivar_data())
            .unwrap();

        db.get_var(RootFsStatus::STATUS_A_PATH)
            .unwrap()
            .write(&RootFsStatus::Normal.to_efivar_data(orb).unwrap())
            .unwrap();
        db.get_var(RootFsStatus::STATUS_B_PATH)
            .unwrap()
            .write(&RootFsStatus::Normal.to_efivar_data(orb).unwrap())
            .unwrap();

        Self {
            _tempdir: tempdir,
            db,
            slot_ctrl,
        }
    }
}
