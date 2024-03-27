# Dependency Tests

One goal of `orb-software` is to support native compilation on as many host
platforms as possible, and to support cross compilation to the orb's
`aarch64-unknown-linux-gnu` platform, *without* needing technologies like
docker. 

This crate helps accomplish this goal in these ways:
- Provide example code of how to properly use these dependencies in the repo.
- Establish a smoke test for these dependencies in CI and guarantee that they
  can cross compile and natively compile on linux.
- Document the platform limitations of various dependencies.

See `Cargo.toml` for canonical documentation for which dependencies support
which targets.

## Adding a new problematic dependency

If you have a dependency you want to add that is problematic:
- Maybe, don't do that 🥺
- Consider using an alternative that natively runs on more platforms, and
  cross-compiles more easily.

If you still really need this dependency, please do the following:
- Add a smoke test for it in this crate.
- You probably need to update the flake.nix to include two versions: the
  native, and the cross architectures. Reach out on slack for help.
- Test cross compilation from both aarch64-apple-darwing (mac m1) and
  x86_64-unknown-linux-gnu.
- Document the platform restrictions as best as possible.
