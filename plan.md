# Orb Monitoring Auth Integration-Test Fixture Plan

## Purpose

Build reusable integration-test infrastructure for `orb-monitoring-auth`.

The fixture will start the real server function with injected dependencies, run the real client code against the server's Unix socket, and replace external infrastructure with isolated test doubles:

- `Clock` uses its existing `faux` support.
- `SecureStorage` uses its existing `faux` support.
- The attestation token comes from an isolated session bus started with `dbus-launch`.
- The monitoring-token backend is represented by WireMock.
- WireMock uses plain HTTP, enabled only in test builds through `orb-security-utils/dangerously-allow-http`.

This plan intentionally avoids adding another server entrypoint or redesigning shutdown. The fixture will call `server::main` directly.

## Repository State at Planning Time

- Current commit: `70442c01 feat(monitoring-auth): multicall bin, placeholder applets`
- `Cargo.lock` already has an uncommitted modification that predates this work.
- Treat the existing `Cargo.lock` change as user-owned. Do not overwrite, revert, or include it without inspecting the exact diff.
- No fixture implementation exists yet.

## User Constraints and Decisions

1. Do not make changes without explicit permission.
2. Keep the implementation direct. Do not introduce extra runtime abstractions.
3. Put the fixture in `orb-monitoring-auth/tests/fixture.rs`.
4. Start the server by calling the existing `async fn main` in `server/mod.rs`.
5. Use `faux` mocks for `Clock` and `SecureStorage`.
6. Use `dbus-launch` for the attestation-token service.
7. Use WireMock for the backend.
8. Enable plain HTTP only in test builds with the existing `dangerously-allow-http` feature.
9. This implementation step creates the fixture infrastructure. Behavioral test cases come afterward.

## Existing Production Flow

The server currently performs these operations:

1. `server::main` loads a monitoring token from `SecureStorage`.
2. It starts `serve_token::task`, which listens on a Unix socket.
3. It starts `refresh_token::task`.
4. The refresh task decides whether a token needs refreshing using `Clock::now()`.
5. When refresh is needed, it obtains the Orb attestation token through D-Bus.
6. It requests a monitoring token from the configured HTTP endpoint.
7. It persists the new token through `SecureStorage::put`.
8. It updates the shared in-memory token served through the Unix socket.
9. The client connects to the socket, reads the token, and emits the Datadog external-auth JSON response.

The integration-test fixture should exercise this production wiring rather than reproduce it.

## Test Topology

```text
integration test
    |
    +-- dbus-launch session bus
    |       |
    |       +-- mock org.worldcoin.AuthTokenManager1 service
    |
    +-- WireMock HTTP server
    |       |
    |       +-- mocked monitoring-token endpoint
    |
    +-- faux Clock
    |
    +-- faux SecureStorage
    |
    +-- server::main(Dependencies)
    |       |
    |       +-- temporary Unix socket
    |
    +-- client::main(current_uid, temporary_socket)
```

Each test will own one fixture. Tests must not share D-Bus daemons, WireMock servers, socket paths, or mock state.

## Scope

### Included

- Test-only dependencies.
- Minimal visibility changes required by an external integration-test crate.
- `orb-monitoring-auth/tests/fixture.rs`.
- Fixture startup, readiness, client invocation, observations, and cleanup.
- Compilation, formatting, and lint verification.

### Excluded

- Behavioral integration-test cases.
- A second server entrypoint.
- A cancellation-token abstraction for the server.
- Signal-handling refactors.
- Client output-capture changes.
- Changes to the production HTTPS policy.
- Changes to the `Token` production API.
- Fixes for unrelated review findings.

## Required File Changes

### 1. `orb-monitoring-auth/Cargo.toml`

Add these development dependencies:

```toml
[dev-dependencies]
async-tempfile.workspace = true
dbus-launch.workspace = true
orb-attest-dbus.workspace = true
wiremock.workspace = true
orb-security-utils = { workspace = true, features = [
    "reqwest",
    "dangerously-allow-http",
] }
```

The crate already depends on `orb-security-utils` with the `reqwest` feature for production. Cargo feature unification will add `dangerously-allow-http` while compiling tests. Normal production builds will continue to enforce HTTPS.

Do not add a general production option for insecure HTTP.

### 2. `orb-monitoring-auth/src/server/mod.rs`

Make two visibility changes.

Change:

```rust
mod secure_storage;
```

to:

```rust
pub mod secure_storage;
```

This lets the external integration-test crate construct `SecureStorage::faux()`.

Change:

```rust
async fn main(deps: Dependencies) -> Result<()>
```

to:

```rust
pub async fn main(deps: Dependencies) -> Result<()>
```

The fixture will spawn this function directly. Do not extract a second entrypoint.

### 3. `orb-monitoring-auth/tests/fixture.rs`

Create the fixture described below.

## Fixture Module Header

The fixture should compile only when the existing test feature is enabled:

```rust
#![cfg(feature = "testing")]
#![allow(dead_code)]
```

The repository's test commands use `--all-features`, so the fixture will compile in CI.

`allow(dead_code)` is appropriate while this file exists before behavioral tests consume every helper. Use the syntactically correct attribute:

```rust
#![allow(dead_code)]
```

## Token Construction

`Token` has private fields but already implements `Serialize`, `Deserialize`, and `Clone`. Avoid changing its production API.

Add a fixture helper:

```rust
pub fn token(value: &str, expiry: DateTime<Utc>) -> Token
```

Construct the token through its serialization contract:

```rust
serde_json::from_value(serde_json::json!({
    "token": value,
    "expiry": expiry.timestamp_millis(),
}))
.expect("fixture token should deserialize")
```

Future tests can inspect tokens by serializing them back into `serde_json::Value`.

## D-Bus Attestation-Token Service

Define a minimal test implementation:

```rust
struct MockAuthTokenManager {
    token: String,
}
```

Implement `orb_attest_dbus::AuthTokenManagerT`:

```rust
impl AuthTokenManagerT for MockAuthTokenManager {
    fn token(&self) -> zbus::fdo::Result<String> {
        Ok(self.token.clone())
    }

    fn force_token_refresh(&mut self, _context: zbus::SignalContext<'_>) {}

    fn new_keys_active(&self) -> zbus::fdo::Result<bool> {
        Ok(false)
    }
}
```

Start the daemon in `spawn_blocking` because `dbus-launch` is blocking:

```rust
let dbusd = tokio::task::spawn_blocking(|| {
    dbus_launch::Launcher::daemon()
        .bus_type(dbus_launch::BusType::Session)
        .launch()
        .expect("failed to launch test D-Bus daemon")
})
.await
.expect("D-Bus launcher task panicked");
```

Build the connection against `dbusd.address()`.

The connection must:

- Claim `org.worldcoin.AuthTokenManager1`.
- Serve at `/org/worldcoin/AuthTokenManager1`.
- Use `orb_attest_dbus::AuthTokenManager::from(mock)`.

Pass this same connection into `server::Dependencies`. Keeping `dbus_launch::Daemon` alive keeps the isolated bus alive.

## WireMock Backend

Start one `wiremock::MockServer` per fixture:

```rust
let backend = wiremock::MockServer::start().await;
```

Expose it before server startup:

```rust
pub fn backend(&self) -> &MockServer
```

Future tests will mount their own request expectations and responses before calling `Fixture::run`.

Use this endpoint in `Dependencies`:

```rust
format!("{}/monitoring-token", backend.uri())
```

The HTTP URL works because test builds enable `orb-security-utils/dangerously-allow-http`.

The fixture should not mount a default backend response. A test that expects refresh must state the expected backend behavior explicitly.

## Faux Clock

Create the existing generated fake:

```rust
let mut clock = Clock::faux();
```

Configure:

```rust
faux::when!(clock.now).then(move |_| now);
```

Use the exact closure shape required by the installed `faux` version. Check existing repository examples if compilation reports a mismatch.

The fixture constructor accepts `now: DateTime<Utc>`, so future tests can place token expiry on either side of the 180-day refresh threshold.

## Faux Secure Storage

Create:

```rust
let mut secure_storage = SecureStorage::faux();
```

The fixture constructor accepts `stored_token: Option<Token>`.

Configure `get()` to return a clone of that value:

```rust
faux::when!(secure_storage.get)
    .then(move |_| Ok(stored_token.clone()));
```

Create shared observation state:

```rust
let persisted_tokens = Arc::new(Mutex::new(Vec::<Token>::new()));
```

Configure `put()` to record a clone:

```rust
let persisted_tokens_for_put = Arc::clone(&persisted_tokens);

faux::when!(secure_storage.put).then(move |token| {
    persisted_tokens_for_put
        .lock()
        .expect("persisted-token lock poisoned")
        .push(token.clone());

    Ok(())
});
```

Again, adjust only the closure argument shape if required by `faux`. Do not replace `faux` with a new storage abstraction.

## `Fixture`

Use a direct struct:

```rust
pub struct Fixture {
    tempdir: async_tempfile::TempDir,
    dbusd: dbus_launch::Daemon,
    dbus: zbus::Connection,
    socket_path: PathBuf,
    backend: wiremock::MockServer,
    clock: Clock,
    secure_storage: SecureStorage,
    persisted_tokens: Arc<Mutex<Vec<Token>>>,
    orb_id: OrbId,
    uid: u32,
}
```

Do not introduce a builder until real test cases demonstrate that one is useful.

## `Fixture::new`

Use this initial API:

```rust
pub async fn new(
    stored_token: Option<Token>,
    now: DateTime<Utc>,
    attestation_token: &str,
) -> Self
```

It should:

1. Create a temporary directory.
2. Create a socket parent under that directory.
3. Set the socket path to a file inside that parent.
4. Start the isolated session bus.
5. Register the D-Bus attestation-token service.
6. Start WireMock.
7. Build the mocked clock.
8. Build the mocked secure storage.
9. Parse a fixed valid `OrbId`.
10. Read the current real UID with `nix::libc::getuid`.
11. Return the prepared fixture without starting the application.

Use a fixed test Orb ID such as one already accepted by `OrbId::from_str`.

Create the socket parent in the fixture. Testing production directory creation is outside this fixture-only scope.

## `Fixture::run`

Use:

```rust
pub async fn run(self) -> FxHandle
```

Destructure `self`, then build:

```rust
let dependencies = Dependencies {
    token_endpoint: format!("{}/monitoring-token", backend.uri()),
    server_socket_path: socket_path.clone(),
    dd_agent_uid: uid,
    clock,
    dbus,
    refresh_token_interval: Duration::from_hours(4),
    orb_id,
    secure_storage,
};
```

Start the production server:

```rust
let server = tokio::spawn(server::main(dependencies));
```

Wait until the socket exists and is a Unix socket before returning.

## Socket Readiness

Add a private helper that polls `tokio::fs::metadata` and uses `std::os::unix::fs::FileTypeExt::is_socket`.

Requirements:

- Use a short overall `tokio::time::timeout`.
- Use a small polling interval.
- Do not use a fixed one-second startup sleep.
- Include the socket path in timeout failures.
- Detect a server task that finishes before creating the socket.

This helper confirms only that the server can accept clients. It does not wait for token refresh.

## `FxHandle`

Use:

```rust
pub struct FxHandle {
    server: tokio::task::JoinHandle<Result<()>>,
    socket_path: PathBuf,
    backend: wiremock::MockServer,
    persisted_tokens: Arc<Mutex<Vec<Token>>>,
    uid: u32,
    _tempdir: async_tempfile::TempDir,
    _dbusd: dbus_launch::Daemon,
}
```

The underscore-prefixed fields retain resource lifetimes.

The D-Bus connection moves into `Dependencies` and remains alive inside the server task.

## Client Invocation

Expose:

```rust
pub async fn run_client(&self) -> Result<()>
```

The client uses blocking `std::os::unix::net::UnixStream`, so call it through `spawn_blocking`:

```rust
let uid = self.uid;
let socket_path = self.socket_path.clone();

tokio::task::spawn_blocking(move || {
    orb_monitoring_auth::client::main(uid, socket_path)
})
.await
.expect("client task panicked")
```

Use the current real UID for both:

- `client::main`'s allowed UID.
- `Dependencies::dd_agent_uid`.

This satisfies the client-side UID check and server-side peer-credential check.

The current client prints its Datadog response and returns `Result<()>`. Do not add stdout capture or change the client API during this fixture-only step.

## Fixture Observations

Expose:

```rust
pub fn backend(&self) -> &MockServer
```

on `Fixture`, allowing backend setup before `run`.

Expose:

```rust
pub fn persisted_tokens(&self) -> Vec<Token>
```

on `FxHandle`, returning a cloned snapshot of tokens passed to `SecureStorage::put`.

Do not return the `Arc<Mutex<_>>` itself.

Also expose:

```rust
pub fn backend(&self) -> &MockServer
```

on `FxHandle`, allowing future tests to inspect requests after the server and client have run.

## Teardown

Provide:

```rust
pub async fn stop(self)
```

The server's existing `main` waits for process signals. The fixture does not need to trigger graceful shutdown.

`stop` should:

1. Abort the server task.
2. Await the aborted task.
3. Allow WireMock, the D-Bus daemon, and the temporary directory to clean themselves up through `Drop`.

Because `FxHandle` implements `Drop`, await the join handle by mutable reference instead of moving it out:

```rust
pub async fn stop(mut self) {
    self.server.abort();
    let _ = (&mut self.server).await;
}
```

Implement `Drop` for `FxHandle` as a panic-safe fallback:

```rust
impl Drop for FxHandle {
    fn drop(&mut self) {
        self.server.abort();
    }
}
```

Do not add production cancellation support.

## Error Handling Style

The fixture may use `expect` and `panic` for setup failures because an infrastructure failure makes the test invalid.

Messages should name the failed dependency or path:

- `"failed to launch test D-Bus daemon"`
- `"failed to register AuthTokenManager test service"`
- `"server exited before creating socket at {path}"`
- `"server did not create socket at {path} before timeout"`

Avoid generic `"setup failed"` messages.

## Future Behavioral Tests

Do not implement these in the fixture step, but design the fixture to support them:

### Cached token

- Secure storage returns a token with more than 180 days remaining.
- Client connects through the Unix socket.
- Backend receives no request.
- Secure storage receives no `put`.

### Expiring token

- Secure storage returns a token with less than 180 days remaining.
- The mocked clock triggers refresh.
- The D-Bus mock supplies the attestation token.
- WireMock verifies the request and returns a new monitoring token.
- Secure storage records the new token.
- The client receives the refreshed token.

### Empty storage

- Secure storage returns `None`.
- The server immediately requests a token.
- The server persists and serves the returned token.

### Backend failure

- Secure storage returns `None`.
- WireMock returns an error.
- No token is persisted.
- The client emits its existing error response.

Each future test must use explicit `Arrange`, `Act`, and `Assert` comments.

## Known Constraints and Risks

### WireMock is HTTP-only

WireMock does not provide TLS. The fixture depends on the existing `dangerously-allow-http` feature, enabled only through a development dependency.

### Client output is not captured

`client::main` prints JSON to stdout and returns `Result<()>`. The fixture can invoke the real client, but exact JSON assertions may require a later, separately reviewed change. Do not solve that while building this fixture.

### Repeated `color_eyre::install`

`client::main` calls `color_eyre::install()`. Repeated client invocations in the same test process may conflict with the global hook. The repository runs tests through `cargo nextest`, which isolates individual tests in separate processes. Do not refactor this preemptively; address it only if the fixture or supported local test command demonstrates a failure.

### Server task abortion

Aborting `server::main` bypasses its signal branch and explicit `speare.abort_children()` call. Confirm during implementation that dropping the root Speare context ends its children and releases the socket. If it does not, report the observed failure before introducing a production shutdown abstraction.

### Existing formatting failure

The reviewed commit already had an unrelated rustfmt failure in `server/refresh_token.rs`. Formatting the fixture must not silently rewrite unrelated files unless the user explicitly authorizes that cleanup.

## Implementation Order

1. Inspect the existing `Cargo.lock` diff and record whether planned dependency changes would overlap it.
2. Add the development dependencies.
3. Make the two visibility changes in `server/mod.rs`.
4. Add the fixture module header and imports.
5. Add the token helper.
6. Add the D-Bus mock and daemon setup.
7. Add the WireMock server.
8. Add the faux clock and secure-storage setup.
9. Implement `Fixture`.
10. Implement `Fixture::run`.
11. Implement socket readiness.
12. Implement `FxHandle`.
13. Implement client invocation and persisted-token observations.
14. Implement teardown.
15. Format only the files changed for this task.
16. Run targeted compilation and lint checks.
17. Inspect the final diff and verify that no unrelated changes entered it.

## Verification

Run:

```text
rustfmt --edition 2024 --check \
    orb-monitoring-auth/src/server/mod.rs \
    orb-monitoring-auth/tests/fixture.rs
cargo check -p orb-monitoring-auth --all-features --tests
cargo clippy -p orb-monitoring-auth --all-features --tests --no-deps -- -D warnings
```

Do not use a workspace-wide formatting command that rewrites the existing unrelated formatting issue.

## Completion Criteria

The fixture work is complete when:

- `tests/fixture.rs` compiles under the `testing` feature.
- The fixture starts an isolated session bus.
- The production D-Bus proxy can retrieve the configured attestation token.
- The test-only HTTP feature allows the production HTTP client to contact WireMock.
- The server reaches Unix-socket readiness.
- `run_client` connects through the real client path.
- Secure-storage writes remain observable.
- Dropping or stopping the handle terminates the server.
- No behavioral tests have been added yet.
- No unrelated files have changed.
- The pre-existing `Cargo.lock` work remains preserved.

## Suggested Skills for the Implementation Session

- `software-testing`
- `software architecture and domain modeling`
- `writing-clearly-and-concisely`

Use `diagnose` only if compilation or runtime behavior fails unexpectedly.

## First Actions in a Fresh Session

1. Read this file completely.
2. Read the repository's current `AGENTS.md`.
3. Check `git status --short`.
4. Inspect the existing `Cargo.lock` diff before running Cargo.
5. Read:
   - `orb-monitoring-auth/Cargo.toml`
   - `orb-monitoring-auth/src/lib.rs`
   - `orb-monitoring-auth/src/client/mod.rs`
   - `orb-monitoring-auth/src/server/mod.rs`
   - `orb-monitoring-auth/src/server/secure_storage.rs`
   - `orb-monitoring-auth/src/server/refresh_token.rs`
   - `orb-backend-status/tests/fixture.rs`
   - `orb-connd/tests/fixture.rs`
6. Confirm that the user has authorized implementation before editing any source file.
