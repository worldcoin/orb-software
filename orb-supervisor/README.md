# Building

To build this crate and run it on the orb, you need to have `zig` installed.
For example through the Arch Linux package manager:

```shell
# pacman -S zig
```

```shell
$ rustup target add aarch64-unknown-linux-gnu
$ cargo install cargo-zigbuild
$ cargo zigbuild --release --target aarch64-unknown-linux-gnu -p orb-supervisor
```

# Running tests

Integration tests are spawned with telemetry, but logs are by default surpressed. To enable logs
in tests run with:

```sh
$ TEST_LOG=1 cargo test
```
