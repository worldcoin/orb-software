# How to develop and build code

Make sure you followed the [first time setup][first time setup] instructions.

## Building

We use `cargo zigbuild` for most things. The following cross-compiles a binary
in the `foobar` crate to the orb. Replace `foobar` with the crate that you wish
to build:

```bash 
cargo zigbuild --target aarch64-unknown-linux-gnu --release -p foobar
```

You can also build code faster with `cargo check`, since it skips the linking
step. Try running `cargo check -p foobar`.

# Testing the code

Unlike building the code, tests are expected to run on the same target as the
host. But not all tests are possible on every target.

*IF* it is supported, you can run `cargo test -p foobar` like normal. But
support varies from crate to crate.

## Running the code

For binaries that are intended to run on the orb, you can take your
cross-compiled binary, and scp it onto the orb. You can either use teleport (if
you have access) via `tsh scp` or you can get the orb's ip address and directly
scp it on, with the `worldcoin` user.

If you choose the `scp` route without teleport, you will need to know the
password for the `worldcoin` user. Note that this password is only for dev
orbs, orbs used in prod are not accessible without teleport and logging in with
a password is disabled.

## Debugging

### Tokio Console

Some of the binaries have support for [tokio console][tokio console]. This is
useful when debugging async code. Arguably the most useful thing to use it for
is to see things like histograms of `poll()` latencies, which can reveal when
one is accidentally blocking in async code. Double check that the binary you
wish to debug actually supports tokio console - support has to be manually
added, it isn't magically available by default.

To use tokio console, you will need to scp a cross-compiled tokio-console
binary to the orb. To do this, just clone the [repo][tokio console] and use
`cargo zigbuild --target aarch64-unknown-linux-gnu --release --bin
tokio-console`, then scp it over.

> Note: tokio-console supports remote debugging via grpc, but I haven't figured out
> how to get the orb to allow that yet - I assume we have a firewall in place
> to prevent arbitrary tcp access, even in dev orbs.

Then, you must build the binary you want to debug unstable tokio features
enabled. To do this, uncomment the line in
[.cargo/config.toml](.cargo/config.toml) about tokio unstable. 

Finally, make sure that the binary has the appropriate RUST_LOG level set up.
try using `RUST_LOG="info,tokio=trace,runtime=trace"`.

Finally, run your compiled binary and the compiled `tokio-console` binary on
the orb. You should see a nice TUI.

Note that it is recommended but not required to have symbols present to improve
the readability of debugging.

[first time setup]: ./first-time-setup.md
[tokio console]: https://github.com/tokio-rs/console?tab=readme-ov-file#extremely-cool-and-amazing-screenshots
