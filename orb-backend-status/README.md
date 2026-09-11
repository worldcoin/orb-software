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

Build and test the Android configuration explicitly:

```sh
cargo build -p orb-backend-status --bin orb-backend-status --no-default-features --features android-collectors --target aarch64-linux-android
cargo test -p orb-backend-status --no-default-features --features android-collectors
```

The Android binary temporarily uses the hardcoded ID `00000000`. This is a
single-device test placeholder, not a production identity. Publishers such as
orb-engine must use the same ID for their Zenoh topic prefix. Once the Android
orb-info implementation is available, replace the placeholder with
`OrbId::read()`; no identity discovery is implemented here.

On Android, run the same binary name with an explicit backend URL and token file:

```sh
./orb-backend-status \
  --endpoint https://BACKEND_HOST/STATUS_PATH \
  --token-file /data/local/tmp/backend-status.token \
  --zenoh-socket /data/local/tmp/zenohd.sock \
  --metrics-socket /data/local/tmp/dsd.socket \
  --orb-os-version android-test
```

Supply the actual status URL and a token authorized for the temporary ID. The
token file contains the token as plain text (a trailing newline is trimmed).
The static token is temporary until Android attestation is available; no token
is embedded in the binary. Name, Jabil ID, and version default to `unknown`;
version can be supplied as shown above.

Logging uses stderr on Android, without journald. Metrics require a local
DogStatsD agent at the configured socket; without it, metrics are not delivered.
The Zenoh router must run separately and listen on the configured UDS path.
Router packaging, orb-engine integration, and device testing remain separate
steps.
