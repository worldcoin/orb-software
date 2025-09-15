use std::fs;

use crate::{
    domain::ScratchRegRetryCount,
    program::{self, Cli},
    EfiRetryCount, EfiVarDb, OrbSlotCtrl, RootFsStatus, Slot,
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
        #[builder(default = 3)] efi_retry_count_a: u8,
        #[builder(default = 3)] efi_retry_count_b: u8,
        #[builder(default = 3)] efi_retry_count_max: u8,
        #[builder(default = 3)] scratch_reg_retry_count_a: u8,
        #[builder(default = 3)] scratch_reg_retry_count_b: u8,
    ) -> Fixture {
        let tempdir = TempDir::new_in("/tmp").unwrap();
        let db_path = tempdir.path().join("sys/firmware/efi/efivars/");
        fs::create_dir_all(&db_path).unwrap();

        let scratch_reg_path = tempdir
            .path()
            .join("sys/devices/platform/bus@0/c360000.pmc/");
        fs::create_dir_all(&scratch_reg_path).unwrap();

        let db = EfiVarDb::from_rootfs(&tempdir).unwrap();
        let slot_ctrl = OrbSlotCtrl::new(&tempdir, orb).unwrap();

        db.get_var(Slot::CURRENT_SLOT_PATH)
            .unwrap()
            .write(&current_slot.to_efivar_data())
            .unwrap();

        db.get_var(Slot::NEXT_SLOT_PATH)
            .unwrap()
            .write(&next_slot.to_efivar_data())
            .unwrap();

        if orb == OrbOsPlatform::Pearl {
            db.get_var(EfiRetryCount::COUNT_A_PATH)
                .unwrap()
                .write(&EfiRetryCount(efi_retry_count_a).to_efivar_data())
                .unwrap();

            db.get_var(EfiRetryCount::COUNT_B_PATH)
                .unwrap()
                .write(&EfiRetryCount(efi_retry_count_b).to_efivar_data())
                .unwrap();
        }

        db.get_var(EfiRetryCount::COUNT_MAX_PATH)
            .unwrap()
            .write(&EfiRetryCount(efi_retry_count_max).to_efivar_data())
            .unwrap();

        db.get_var(RootFsStatus::STATUS_A_PATH)
            .unwrap()
            .write(&status_a.to_efivar_data(orb).unwrap())
            .unwrap();

        db.get_var(RootFsStatus::STATUS_B_PATH)
            .unwrap()
            .write(&status_b.to_efivar_data(orb).unwrap())
            .unwrap();

        if orb == OrbOsPlatform::Diamond {
            fs::write(
                tempdir
                    .path()
                    .join(ScratchRegRetryCount::DIAMOND_COUNT_A_PATH),
                format!("0x{scratch_reg_retry_count_a}\n"),
            )
            .unwrap();

            fs::write(
                tempdir
                    .path()
                    .join(ScratchRegRetryCount::DIAMOND_COUNT_B_PATH),
                format!("0x{scratch_reg_retry_count_b}\n"),
            )
            .unwrap();
        }

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
