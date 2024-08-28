# Coding Guidelines

When contributing code, keep the following guidelines in mind.

- Code must pass CI - see the github actions workflow for the most up to date
  checks.
- There can be no copyleft or closed source dependencies.
- Prefer using cargo [workspace inheritance] when possible.
- Prefer cross-platform code. Please consult [deps tests][deps tests] for more
  info.
- All crates and binaries must support at least
  `{x86_64-aarch64}-unknown-linux-gnu` as a compilation target. Any other
  targets which are not supported must be specified in
  `package.metadata.orb.unsupported_targets`. Windows is implicitly not
  supported as a compilation target.
- Use `#![forbid(unsafe_code)]` whenever possible. This narrows the surface
  area for debugging memory safety issues.
- Prefer the [rustix crate][rustix crate] for safe unix APIs instead of raw unsafe
  libc. PRs that use `libc` will be rejected if an equivalent safe function in
  `rustix` exists.
- PR names and the final squashed commit that gets merged, should start with an
  area prefix, like `ir-camera:`. This helps disambiguate which part of the
  monorepo changed at a glance.
- All first-party crates should start with the `orb-` prefix for the crate
  name, and the crates' directories should omit this prefix. For example, the
  `attest` dir contains the `orb-attest` crate.
- All binaries intended for deployment to orbs, should have a .deb produced by
  CI. CI will produce any such .deb for crates with a `package.metadata.deb`
  section in the Cargo.toml.

[workspace inheritance]: https://doc.rust-lang.org/cargo/reference/workspaces.html#the-package-table
[deps tests]: https://github.com/worldcoin/orb-software/tree/main/deps-tests
[rustix crate]: https://docs.rs/rustix
