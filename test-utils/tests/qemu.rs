use cmd_lib::run_cmd;
use test_utils::qemu::{self, base};

#[cfg_attr(target_os = "macos", test_with::no_env(GITHUB_ACTIONS))]
#[test]
fn run_in_parallel_one() {
    let q = qemu::fx::run("/tmp", &base::bullseye());

    run_cmd!(echo blabla > /tmp/hello).unwrap();
    q.copy("/tmp/hello", "/home/worldcoin/hello");

    let guest_hello = q.run("cat /home/worldcoin/hello");

    assert_eq!(guest_hello, "blabla");
}

#[cfg_attr(target_os = "macos", test_with::no_env(GITHUB_ACTIONS))]
#[test]
fn run_in_parallel_two() {
    let q = qemu::fx::run("/tmp", &base::bullseye());

    run_cmd!(echo blabla > /tmp/hello).unwrap();
    q.copy("/tmp/hello", "/home/worldcoin/hello");

    let guest_hello = q.run("cat /home/worldcoin/hello");

    assert_eq!(guest_hello, "blabla");
}
