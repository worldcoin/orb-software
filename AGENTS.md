# Repository Guidelines

## Project Structure & Module Organization
- Monorepo: roughly each top-level directory is a component/crate (see `Cargo.toml [workspace]`). Examples: `attest/`, `supervisor/`, `update-agent/`, `ui/`.
- Shared configs: `rust-toolchain.toml`, `rustfmt.toml`, `deny.toml`, `flake.nix`, `.envrc.example`.
- Docs: `docs/` (mdBook with `book.toml` and `src/`). CI: `.github/workflows/`. Scripts: `scripts/`.
- Typical crate layout: `src/` for code, optional `tests/` for integration tests.

## Build, Test, and Development Commands
- Enter dev env: use direnv or `nix develop`.
  - One-off: `nix develop -c cargo --version`
- Build (host): `cargo build -p <crate>`; quick checks: `cargo check -p <crate>`.
- Cross-build for Orb: `cargo zigbuild --target aarch64-unknown-linux-gnu --release -p <crate>`.
- Test (workspace): `cargo test --all --all-targets` or per-crate: `cargo test -p <crate>`. This will not work sometimes on macos, in which case narrow the set of crates via `-p`.
- Lint: `cargo clippy --all --all-features --all-targets -- -D warnings`.
- Format: `cargo fmt --all` (CI enforces `--check`).
- Licenses/advisories: `cargo deny check licenses` and `cargo deny check advisories`.

## Coding Style & Naming Conventions
- Rust edition 2024; formatting via `rustfmt` (see `rustfmt.toml`, `max_width = 88`).
- Prefer `#![forbid(unsafe_code)]` and safe Unix APIs via `rustix` instead of `libc`.
- First-party crate names start with `orb-`; directory names omit the prefix (e.g., dir `attest/` => crate `orb-attest`).
- Avoid copyleft dependencies; see `deny.toml` allowlist and exceptions.
- Do not use `Arc<tokio::sync::Mutex<T>>`, instead favor either a `Arc<std::sync::Mutex<T>>`, or use message passing via tokio tasks and channels.
- Try to avoid async-trait macros, instead prefer using regular async traits (built into rust) and use an Enum instead of a trait object. Alternatively, use the dynosaur crate.
- All CLIs should use the `clap` crate, follow the examples in the `orb-telemetry` crate in the workspace for how to set up telemetry and use `orb-build-info` for the crate version.
- Ensure that you don't ever call code that would block the thread from an asynchronous function.
- Avoid OOP style code. Prefer using composition and Rust's data types (structs, enums).
- Try to avoid traits when possible, unless it is necessary for testability.
- Avoid writing code when an existing library will do the job.
- All configuration should be configured in the entry point of the software, and passsed into the rest of the program as explicit config structs via dependency injection.
- Do not rely on global state like environment variables - reading environment variables should only happen in the `main` of the program, if at all.
- When writing HTTP services, use the axum crate along with sqlx if you need a database. Prefer sqlx's sqlite backend when possible, especially for local tests. Be sure to make use of sqlx's database migration feature. Be sure to keep request and response types strongly typed, via axum's existing `Json<T>` type.
- leverage `tower::ServiceExt::oneshot` for testing of axum routers. Check axum's github for example code of how to do this.
- When planning on how to write the code, its important to design it in a way that makes it testable.
- Use rust's `tracing` and `metrics` crates for logging, and be sure to utilize tracing spans to associate log messages with a particular span/event via the `#[instrument]` macro where appropriate.
- Use newtypes when possible, for increased type safety. Generally bias towards exposing the inner type via a pub field, for simplicity, and to avoid duplicating the API surface.
- If you want to script some stuff on the CLI, consider using the `cmd_lib` crate to make spawning subcommands more terse.
- Keep code simple. Do not write code that overly abstracts things. Encapsulating existing types from libraries behind a new first-party struct or interface is a bad idea and leads to complexity.
- When writing code that performs IPC over dbus, utilize the zbus crate. First implement your API in a strongly typed fashion as an orb-foobar-dbus crate, which both the interface (server) and proxy (client) will depend on. See how this is done in `orb-attest-dbus` as an example. The `orb-*-dbus` crate should have roughly no dependencies, instead it accepts the implementation details via dependency injection.
- Be sure that when initializing dbus sessions, you always pass it in as configuration, and instantiate it from main. This allows tests to instantiate it a different way, via the `dbus-launch` crate. You can then initialize a test-specific dbus bus, pass the location of that to zbus via zbus's connection builder, and then since the rest of the program has the zbus connection passed in as configuration from `main`, it becomes trivial for the tests to override the connection and ensure tests have isolated busses. 

## Testing Guidelines
- Use standard Rust tests: unit tests in modules, integration tests under `tests/`.
- Run locally with `cargo test`; some crates are Linux-only, test per-crate when on macOS.
- Leverage rust's testcontainers library and things like minio or aws localstack if minio doesn't work.
- Containers and cross-test options are documented in `docs/src/development.md`.

## Commit & Pull Request Guidelines
- PR titles must follow Conventional Commits (validated in CI). Include an area prefix when helpful, e.g., `feat(hil): added foobar`, where `hil` is the area.
- PRs require a non-empty description. Link issues, include logs or screenshots for UI changes.
- Keep changes focused and pass CI (fmt, clippy, tests, cargo-deny).

## Security & Configuration Tips
- Use the Nix/direnv environment (`.envrc`) and follow `docs/src/first-time-setup.md` to vendor required SDKs. This is typically already done by the user.
- Never add closed-source or copyleft deps outside documented exceptions.
- For cross-compiles and production artifacts, prefer `cargo zigbuild` and the provided CI workflows.

