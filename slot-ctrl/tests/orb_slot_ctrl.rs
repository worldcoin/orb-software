use orb_slot_ctrl::test_utils::Fixture;
use orb_slot_ctrl::{RootFsStatus, Slot};

#[test]
fn it_gets_current_slot() {
    let fx = Fixture::new(Slot::A, 5);
    let slot = fx.slot_ctrl.get_current_slot().unwrap();
    assert_eq!(slot, Slot::A)
}

#[test]
fn it_gets_inactive_slot() {
    let fx = Fixture::new(Slot::B, 5);
    let slot = fx.slot_ctrl.get_inactive_slot().unwrap();
    assert_eq!(slot, Slot::A)
}

#[test]
fn it_gets_and_sets_next_boot_slot() {
    let fx = Fixture::new(Slot::B, 5);
    let slot = fx.slot_ctrl.get_next_boot_slot().unwrap();
    assert_eq!(slot, Slot::B);

    fx.slot_ctrl.set_next_boot_slot(Slot::A).unwrap();
    let slot = fx.slot_ctrl.get_next_boot_slot().unwrap();
    assert_eq!(slot, Slot::A)
}

#[test]
fn it_gets_and_sets_current_rootfs_status() {
    let fx = Fixture::new(Slot::A, 5);
    let status = fx.slot_ctrl.get_current_rootfs_status().unwrap();
    assert_eq!(status, RootFsStatus::Normal);

    fx.slot_ctrl
        .set_current_rootfs_status(RootFsStatus::Unbootable)
        .unwrap();

    let status = fx.slot_ctrl.get_current_rootfs_status().unwrap();
    assert_eq!(status, RootFsStatus::Unbootable)
}

#[test]
fn it_gets_and_sets_current_rootfs_status_on_specific_slot() {
    let fx = Fixture::new(Slot::A, 5);
    let status = fx.slot_ctrl.get_rootfs_status(Slot::B).unwrap();
    assert_eq!(status, RootFsStatus::Normal);

    fx.slot_ctrl
        .set_rootfs_status(RootFsStatus::Unbootable, Slot::B)
        .unwrap();

    let status = fx.slot_ctrl.get_rootfs_status(Slot::B).unwrap();
    assert_eq!(status, RootFsStatus::Unbootable)
}

#[test]
fn it_sets_fw_status() {
    let fx = Fixture::new(Slot::A, 5);
    // Just verify that this doesn't panic
    fx.slot_ctrl.set_fw_status(0).unwrap();
}
