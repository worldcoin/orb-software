# orb-backend-status

This daemon provide a dbus sink to collect status from varous orb systems and periodically provide them to the Orb Fleet Backend.

If you are on mac, be sure that you installed dbus. See the toplevel [README.md]

## Collector features

`linux-collectors` is the default and preserves the existing service. It also
takes precedence with `--all-features`, as used by `cargo x t`. Build and test the Android configuration explicitly:

```sh
cargo build -p orb-backend-status --bin orb-backend-status --no-default-features --features android-collectors --target aarch64-linux-android
cargo test -p orb-backend-status --no-default-features --features android-collectors
```
