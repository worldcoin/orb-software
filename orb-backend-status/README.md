# orb-backend-status

This daemon provide a dbus sink to collect status from varous orb systems and periodically provide them to the Orb Fleet Backend.

If you are on mac, be sure that you installed dbus. See the toplevel [README.md]

## Collector features

`linux-collectors` is the default and preserves the existing service. It also
takes precedence with `--all-features`, as used by `cargo x t`.

The Android collector implementation currently supplies a static token through
`collectors::Config` and empty platform snapshots. It starts no platform reporters
or subscriptions; OES consumption, caching, flushing, and HTTP sending remain
shared. Without a connectivity collector, Android allows HTTP attempts directly;
this is not a backend health signal. Linux-only fields keep the same JSON schema
and remain absent or null until Android collectors populate them.

Check and test this library configuration explicitly:

```sh
cargo check -p orb-backend-status --lib --no-default-features --features android-collectors --target aarch64-linux-android
cargo test -p orb-backend-status --lib --no-default-features --features android-collectors
```

The binary still requires `linux-collectors` in this intermediate step. Android
entry-point configuration, SoC identity wiring, and router/device deployment are
not implemented yet. The static token is a temporary input until Android
attestation is available; no token is embedded in the library.
