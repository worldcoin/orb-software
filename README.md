# orb-update-verifier
Checks general system health and manages the slot and rootfs state of the Orb.
It is designed to run as systemd oneshot service that will run once on boot.

## Systemd and integration
The systemd service configuration can be found in `debpkg/lib/systemd/system/worldcoin-update-verifier.service` and 
will be packaged together with the binary in CI.

## Building

### Prerequisites
+ `rustup`: `1.25.2` (tested with 1.25, might work with older versions)
+ `rustc`: `1.67.0`
+ `ziglang`: `0.10.1`

Using Arch Linux as an example, you can install `ziglang` and `rustup` using pacman, and in turn
get the most recent version of `rustc`:

```sh
$ sudo pacman -S zig rustup
$ rustup install stable
```

```sh
$ brew install zig rustup-init
$ rustup-init
```

The easiest way to cross-compile for the orb is to use `cargo-zigbuild`, which
in turn relies on ziglang's tooling to act as a linker.

```sh
$ rustup target add aarch64-unknown-linux-gnu
$ cargo install cargo-zigbuild
```

### Compiling

```sh
$ cargo zigbuild --release --target aarch64-unknown-linux-gnu.2.27
```

### Testing

Health test can be forced by setting environment variable `UPDATE_VERIFIER_DRY_RUN`.

```sh
$ sudo UPDATE_VERIFIER_DRY_RUN="1" ./update-verifier
```
