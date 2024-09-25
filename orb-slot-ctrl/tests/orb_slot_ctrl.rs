use orb_slot_ctrl::test_utils::Fixture;
use orb_slot_ctrl::{RootFsStatus, Slot};

#[test]
fn it_gets_current_slot() {
    let fx = Fixture::new();
    let slot = fx.slot_ctrl.get_current_slot().unwrap();
    assert_eq!(slot, Slot::A)
}

#[test]
fn it_gets_inactive_slot() {
    let fx = Fixture::new();
    let slot = fx.slot_ctrl.get_inactive_slot().unwrap();
    assert_eq!(slot, Slot::B)
}

#[test]
fn it_gets_and_sets_next_boot_slot() {
    let fx = Fixture::new();
    let slot = fx.slot_ctrl.get_next_boot_slot().unwrap();
    assert_eq!(slot, Slot::B);

    fx.slot_ctrl.set_next_boot_slot(Slot::A).unwrap();
    let slot = fx.slot_ctrl.get_next_boot_slot().unwrap();
    assert_eq!(slot, Slot::A)
}

#[test]
fn it_gets_and_sets_current_rootfs_status() {
    let fx = Fixture::new();
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
    let fx = Fixture::new();
    let status = fx.slot_ctrl.get_rootfs_status(Slot::B).unwrap();
    assert_eq!(status, RootFsStatus::Normal);

    fx.slot_ctrl
        .set_rootfs_status(RootFsStatus::Unbootable, Slot::B)
        .unwrap();

    let status = fx.slot_ctrl.get_rootfs_status(Slot::B).unwrap();
    assert_eq!(status, RootFsStatus::Unbootable)
}

#[test]
fn it_gets_and_resets_current_retry_count_to_max() {
    let fx = Fixture::new();
    let count = fx.slot_ctrl.get_current_retry_count().unwrap();
    assert_eq!(count, 0);

    fx.slot_ctrl.reset_current_retry_count_to_max().unwrap();
    let count = fx.slot_ctrl.get_current_retry_count().unwrap();
    assert_eq!(count, 5);
}

#[test]
fn it_gets_and_resets_current_retry_count_to_max_on_specific_slot() {
    let fx = Fixture::new();
    let count = fx.slot_ctrl.get_retry_count(Slot::B).unwrap();
    assert_eq!(count, 0);

    fx.slot_ctrl.reset_retry_count_to_max(Slot::B).unwrap();
    let count = fx.slot_ctrl.get_retry_count(Slot::B).unwrap();
    assert_eq!(count, 5);
}
