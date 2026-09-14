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
- The manifest name, init script, and SELinux context are still
  placeholders (see TODOs in `xtask/src/cmd/android.rs`) - the `.apex`
  installs fine, but the packaged daemon won't actually start under
  init on a real device yet.
