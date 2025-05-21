use std::fs;

use crate::{EfiVarDb, OrbSlotCtrl, Slot};
use orb_info::orb_os_release::OrbType;
use tempfile::TempDir;

/// A Fixture that initializes fake EfiVars.
/// Both Rootfs slots are normal by default.
pub struct Fixture {
    _tempdir: TempDir,
    pub db: EfiVarDb,
    pub slot_ctrl: OrbSlotCtrl,
    // pub rootfs: RootfsEfiVars,
}

impl Fixture {
    pub fn new(current_and_next_slot: Slot, max_retry_count: u8) -> Self {
        let tempdir = TempDir::new_in("/tmp").unwrap();
        let db_path = tempdir.path().join("sys/firmware/efi/efivars/");
        fs::create_dir_all(&db_path).unwrap();

        let db = EfiVarDb::from_rootfs(&tempdir).unwrap();
        // let bootchain = BootChainEfiVars::new(&db).unwrap();
        // let rootfs = RootfsEfiVars::new(&db).unwrap();

        let orb_type = OrbType::Pearl;
        let slot_ctrl = OrbSlotCtrl::from_evifar_db(&db, orb_type).unwrap();

        let slot = match current_and_next_slot {
            Slot::A => 0x00,
            Slot::B => 0x01,
        };

        // bootchain
        //     .current
        //     .write(&[0x07, 0x00, 0x00, 0x00, slot, 0x00, 0x00, 0x00])
        //     .unwrap();

        // Initialize next boot slot to assumed default value from Efi
        // bootchain
        //     .next
        //     .write(&[0x07, 0x00, 0x00, 0x00, slot, 0x00, 0x00, 0x00])
        //     .unwrap();

        // rootfs
        //     .retry_count_a
        //     .write(&[0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00])
        //     .unwrap();

        // rootfs
        //     .retry_count_b
        //     .write(&[0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00])
        //     .unwrap();

        // rootfs
        //     .retry_count_max
        //     .write(&[0x07, 0x00, 0x00, 0x00, max_retry_count, 0x00, 0x00, 0x00])
        //     .unwrap();

        // rootfs
        //     .status_a
        //     .write(&[0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00])
        //     .unwrap();

        // rootfs
        //     .status_b
        //     .write(&[0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00])
        //     .unwrap();

        Self {
            _tempdir: tempdir,
            db,
            slot_ctrl,
            // rootfs,
        }
    }
}
