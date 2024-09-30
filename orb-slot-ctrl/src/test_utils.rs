use std::fs;

use crate::{
    efivar::{bootchain::BootChainEfiVars, rootfs::RootfsEfiVars},
    EfiVarDb, OrbSlotCtrl,
};
use tempfile::TempDir;

pub struct Fixture {
    _tempdir: TempDir,
    pub db: EfiVarDb,
    pub slot_ctrl: OrbSlotCtrl,
    pub bootchain: BootChainEfiVars,
    pub rootfs: RootfsEfiVars,
}

impl Default for Fixture {
    fn default() -> Self {
        Self::new()
    }
}

impl Fixture {
    pub fn new() -> Self {
        let tempdir = TempDir::new_in("/tmp").unwrap();
        let db_path = tempdir.path().join("sys/firmware/efi/efivars/");
        fs::create_dir_all(&db_path).unwrap();

        let db = EfiVarDb::from_rootfs(&tempdir).unwrap();
        let bootchain = BootChainEfiVars::new(&db).unwrap();
        let rootfs = RootfsEfiVars::new(&db).unwrap();
        let slot_ctrl = OrbSlotCtrl::new(&db).unwrap();

        bootchain
            .current
            .create_and_write(&[0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00])
            .unwrap();

        // Initialize next boot slot to assumed default value from Efi
        bootchain
            .next
            .create_and_write(&[0x07, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00])
            .unwrap();

        rootfs
            .retry_count_a
            .create_and_write(&[0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00])
            .unwrap();

        rootfs
            .retry_count_b
            .create_and_write(&[0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00])
            .unwrap();

        rootfs
            .retry_count_max
            .create_and_write(&[0x07, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00])
            .unwrap();

        rootfs
            .status_a
            .create_and_write(&[0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00])
            .unwrap();

        rootfs
            .status_b
            .create_and_write(&[0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00])
            .unwrap();

        Self {
            _tempdir: tempdir,
            db,
            slot_ctrl,
            bootchain,
            rootfs,
        }
    }
}
