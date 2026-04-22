use orb_info::orb_os_release::OrbOsPlatform;
use orb_slot_ctrl::test_utils::Fixture;
use orb_slot_ctrl::{RootFsStatus, Slot};

#[test]
fn it_gets_current_slot() {
    let fx = Fixture::builder()
        .current_slot(Slot::A)
        .build(OrbOsPlatform::Pearl);

    let slot = fx.run("current").unwrap();
    assert_eq!(slot, "a")
}

#[test]
fn it_gets_inactive_slot() {
    let fx = Fixture::builder()
        .current_slot(Slot::B)
        .build(OrbOsPlatform::Pearl);

    let slot = fx.slot_ctrl.get_inactive_slot().unwrap();
    assert_eq!(slot, Slot::A)
}

#[test]
fn it_gets_and_sets_next_boot_slot_marking_it_as_normal() {
    let fx = Fixture::builder()
        .current_slot(Slot::A)
        .next_slot(Slot::A)
        .status_a(RootFsStatus::Normal)
        .status_b(RootFsStatus::Unbootable)
        .build(OrbOsPlatform::Pearl);

    let next = fx.run("next").unwrap();
    assert_eq!(next, "a");

    let status = fx.run("status -i get").unwrap();
    assert_eq!(status, "Unbootable");

    fx.run("set b").unwrap();
    let next = fx.run("next").unwrap();
    assert_eq!(next, "b");

    let status = fx.run("status -i get").unwrap();
    assert_eq!(status, "Normal");
}

#[test]
fn it_gets_sets_and_deletes_bootchain_fw_status() {
    let fx = Fixture::builder()
        .current_slot(Slot::B)
        .build(OrbOsPlatform::Pearl);

    fx.run("bootchain-fw set 0").unwrap();
    let status = fx.run("bootchain-fw get").unwrap();
    assert_eq!(status, "Success");

    fx.run("bootchain-fw delete").unwrap();
    let status = fx.run("bootchain-fw get");
    assert!(status.is_err());
}

#[test]
fn it_gets_and_sets_rootfs_status() {
    // Current slot
    let fx = Fixture::builder()
        .current_slot(Slot::A)
        .status_a(RootFsStatus::Normal)
        .status_b(RootFsStatus::Normal)
        .build(OrbOsPlatform::Pearl);

    let status = fx.run("status get").unwrap();
    assert_eq!(status, "Normal");

    fx.run("status set 3").unwrap();
    let status = fx.run("status get").unwrap();
    assert_eq!(status, "Unbootable");

    // Inactive slot
    let inactive_status = fx.run("status -i get").unwrap();
    assert_eq!(inactive_status, "Normal");

    fx.run("status -i set 3").unwrap();
    let inactive_status = fx.run("status -i get").unwrap();
    assert_eq!(inactive_status, "Unbootable");
}

#[test]
fn it_marks_slot_ok() {
    let fx = Fixture::builder()
        .current_slot(Slot::A)
        .status_a(RootFsStatus::UpdateInProcess)
        .efi_retry_count_a(1)
        .efi_retry_count_max(3)
        .sr_rf_retry_count_a(1)
        .build(OrbOsPlatform::Pearl);

    // Setup validation
    fx.run("bootchain-fw set 0").unwrap();
    assert_eq!(fx.run("status get").unwrap(), "UpdateInProcess");
    assert_eq!(fx.run("status max").unwrap(), "3");
    assert_eq!(fx.run("status retries").unwrap(), "efi var: 1\nSR_RF: 1\n");
    assert_eq!(fx.run("bootchain-fw get").unwrap(), "Success");

    // Execution
    fx.slot_ctrl.mark_slot_ok(Slot::A).unwrap();

    // Assertions
    assert_eq!(fx.run("status get").unwrap(), "Normal");
    assert_eq!(fx.run("status retries").unwrap(), "efi var: 3\nSR_RF: 3\n");
    assert!(fx.run("bootchain-fw get").is_err());
}

#[test]
fn it_marks_slot_ok_deletes_bootchain_fw_status_if_present() {
    // marking slot as ok deletes BootChainFwStatus if its there, and change
    // status to Normal
    let fx = Fixture::builder()
        .current_slot(Slot::A)
        .status_a(RootFsStatus::Normal)
        .status_b(RootFsStatus::Unbootable)
        .build(OrbOsPlatform::Diamond);

    let status = fx.run("status -i get").unwrap();
    assert_eq!(status, "Unbootable");

    fx.run("bootchain-fw set 0").unwrap();
    let status = fx.run("bootchain-fw get").unwrap();
    assert_eq!(status, "Success");

    fx.slot_ctrl.mark_slot_ok(Slot::B).unwrap();
    let failed = fx.run("bootchain-fw get");
    assert!(failed.is_err());

    let status = fx.run("status -i get").unwrap();
    assert_eq!(status, "Normal");
}
