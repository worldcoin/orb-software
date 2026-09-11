# Android

Cross-compiling this workspace to `aarch64-linux-android`, and packaging
binaries into signed `.apex` files.

## Setup

Enter the dev shell (`nix develop` or direnv) - it wires up the NDK
toolchain env vars automatically. Nothing else to install.

## Build for Android

```sh
cargo x android-build          # debug
cargo x android-build --release
```

Builds the whole workspace for `aarch64-linux-android`, skipping crates
that don't support it (see `[package.metadata.orb] unsupported_targets`
in their `Cargo.toml` - usually dbus/systemd/gstreamer-dependent crates).

## Standalone Zenoh router

The upstream router is an explicit target of the same build/APEX commands:

```sh
cargo x android-build zenohd
cargo x android-apex zenohd
```

Add `--release` to either command for a release build. `zenohd` is not a
workspace crate and is not included by the no-argument workspace build.
`android-test` and `android-clippy` still operate only on workspace crates.

The build installs upstream `zenohd` **1.7.2**, matching the workspace's locked
Zenoh version, into the workspace target directory, not the user's Cargo bin
directory. It uses the upstream `Cargo.lock` (`--locked`) and disables default
features, enabling only `zenoh/transport_unixsock-stream`. No router fork,
storage plugin, REST plugin, or additional delivery mechanism is introduced.
The upstream lock currently produces Cargo warnings for yanked dependencies;
this pinned test build is not a production dependency qualification.

Artifacts (relative to Cargo's target directory):

- Binary: `android-tools/zenohd/1.7.2/debug/bin/zenohd` (`release` for release builds).
- Package: `android-apex/com.toolsforhumanity.zenohd.apex` by default.
- Packaged configuration: `/apex/com.toolsforhumanity.zenohd/etc/zenohd.json5`.

The config at `android/zenohd/zenohd.json5` listens only on
`unixsock-stream//data/local/tmp/zenohd.sock`. Multicast and gossip discovery are
disabled. This matches backend-status's current `--zenoh-socket` default.
The router does not need an Orb ID; publisher and subscriber clients both use
the temporary `00000000` topic prefix.

The router APEX is **manual-start only** until the firmware has a dedicated
SELinux domain. After installation/activation, run in a device shell:

```sh
RUST_LOG=warn /apex/com.toolsforhumanity.zenohd/bin/zenohd \
  --config /apex/com.toolsforhumanity.zenohd/etc/zenohd.json5
```

Keep the router running while starting backend-status and the publisher.
The socket directory must exist and the router's user/domain must be able to
create both the socket and its `.lock` file. Clients need access to the socket.
The `/data/local/tmp` path is for manual device testing, not the final service
data directory. Do not remove a live router's socket or lock file.

`cargo test -p zenorb --test uds` tests the packaged config's UDS routing with
two native client sessions and checks that an unusable socket path fails.
It runs on the host, not an Android device, and does not test Android policy.

### SELinux labels and process domains

SELinux is an additional permission check beyond Unix user/group/mode bits.
A **domain** is the SELinux type attached to a process. For a label such as
`u:r:orb_zenohd:s0`, `orb_zenohd` is the domain. Files and sockets have object
types, for example `u:object_r:system_file:s0`. Policy rules describe which
domains may access which object types and perform which operations.
See [AOSP SELinux concepts](https://source.android.com/docs/security/features/selinux/concepts).

This package labels its files `system_file`, an existing Android file type.
It does **not** define an `orb_zenohd` process domain or ship `init.rc`.
When started manually without a policy transition, the router retains the
launcher's domain (often `shell` for an ordinary ADB shell; root/debug setups
can differ). The file label and UID do not prove what the process domain is.
Check the actual device rather than assuming:

```sh
id -Z
getenforce
ps -AZ | grep zenohd
ls -Z /data/local/tmp/zenohd.sock /data/local/tmp/zenohd.sock.lock
```

For an init-managed service in enforcing mode, the **firmware policy** needs:

- A router domain and executable type, plus the init-to-router transition.
- A writable directory/file type for the socket and regular lock file.
- Router permission to create/listen on the Unix stream socket.
- Client permissions for the directory/socket file and `connectto` permission
  to the router domain's `unix_stream_socket`.

Those permissions must cover the actual orb-engine and backend-status domains.
An APEX `file_contexts` file assigns object labels; it cannot define new types
or grant these permissions on its own. Adding `seclabel` to an init service
also requires the domain and transition permissions to exist in the firmware.
See [AOSP SELinux implementation](https://source.android.com/docs/security/features/selinux/implement).

The local orb-engine `apex.nix` currently uses `mediaserver_exec` for its daemon
and invokes `setenforce 0`; that is a development workaround, not a dedicated
orb-engine/router policy. This router package does not copy it, disable
enforcement, or change firmware policy. With missing policy, enforcing-mode
access can still fail; inspect AVC denials before defining the final rules.

The existing `android-deploy` command can disable **dm-verity**, remount vendor,
and reboot when seeding a previously unseen APEX. That is separate from SELinux
and is not performed by `android-build` or `android-apex`. Device deployment
remains a separate, explicitly requested step.

## Package into `.apex`

```sh
cargo x android-apex               # every crate
cargo x android-apex orb-foo       # just one crate
cargo x android-apex --release
```

Requires an x86_64-linux host and `nix` on `PATH` (fetches/builds the
`build-apex` flake package on first run). Output lands in
`target/android-apex/<crate>.apex`, signed with AOSP's public test
key - never a real release signature.

## Install onto a device

```sh
cargo x android-deploy               # every crate
cargo x android-deploy orb-foo       # just one crate
cargo x android-deploy orb-foo --release
```

Runs `android-apex` under the hood, then installs each resulting `.apex` via
`adb install -t -r -g --force-non-staged`, so it's usable immediately - no
reboot required.
Needs a device reachable over `adb` (same x86_64-linux + `nix`
requirement as `android-apex` also applies here).

## Gotchas

- Not every crate builds for Android.
- For workspace crates, the init script and SELinux context are still
  placeholders (see TODOs in `xtask/src/cmd/apex.rs`) - the `.apex`
  installs fine, but the packaged daemon won't actually start under
  init on a real device yet. The upstream router is explicitly manual-start
  instead, as described above.
