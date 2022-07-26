# Building

To build this crate and run it on the orb, make sure to have `zig` installed.
For example through the Arch Linux package manager:

```shell
# pacman -S zig
```

```shell
$ rustup target add aarch64-unknown-linux-gnu
$ cargo install cargo-zigbuild
$ cargo zigbuild --release --target aarch64-unknown-linux-gnu -p orb-supervisor
```
