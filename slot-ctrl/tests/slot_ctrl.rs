use orb_info::orb_os_release::OrbType;
use orb_slot_ctrl::test_utils::Fixture;
use orb_slot_ctrl::{RootFsStatus, Slot};

#[test]
fn it_gets_current_slot() {
    let fx = Fixture::builder()
        .current_slot(Slot::A)
        .build(OrbType::Pearl);

    let slot = fx.run("current").unwrap();
    assert_eq!(slot, "a")
}

#[test]
fn it_gets_inactive_slot() {
    let fx = Fixture::builder()
        .current_slot(Slot::B)
        .build(OrbType::Pearl);

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
        .build(OrbType::Pearl);

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
        .build(OrbType::Pearl);

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
        .build(OrbType::Pearl);

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
fn it_marks_slot_ok_on_pearl() {
    let fx = Fixture::builder()
        .current_slot(Slot::A)
        .status_a(RootFsStatus::UpdateInProcess)
        .retry_count_a(1)
        .retry_count_max(9)
        .build(OrbType::Pearl);

    // on Pearl marking slot as ok resets retry count and marks slot as Normal
    let status = fx.run("status get").unwrap();
    assert_eq!(status, "UpdateInProcess");
    let status = fx.run("status retries").unwrap();
    assert_eq!(status, "1");
    let status = fx.run("status max").unwrap();
    assert_eq!(status, "9");

    fx.slot_ctrl.mark_slot_ok(Slot::A).unwrap();
    let status = fx.run("status get").unwrap();
    assert_eq!(status, "Normal");
    let status = fx.run("status retries").unwrap();
    assert_eq!(status, "9");
}

#[test]
fn it_marks_slot_ok_on_diamond_deletes_bootchain_fw_status_if_present() {
    // on Diamond marking slot as ok deletes BootChainFwStatus if its there, and change
    // status to Normal
    let fx = Fixture::builder()
        .current_slot(Slot::A)
        .status_a(RootFsStatus::Normal)
        .status_b(RootFsStatus::Unbootable)
        .build(OrbType::Diamond);

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
