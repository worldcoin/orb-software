# orb-pcp

Builds Personal Custody Packages (PCP 2.8): payload encoding, Hyrax
commitments, archive layout, hashing, signing, compression and alkali/libsodium
sealed-box encryption.

## Build and example

Use the repository development environment, which provides the pinned Rust
toolchain, `protoc`, `pkg-config` and libsodium.

```sh
cargo build -p orb-pcp --example build_pcp
cargo run -p orb-pcp --example build_pcp
cargo test -p orb-pcp --all-features --all-targets
```

[examples/build_pcp.rs](examples/build_pcp.rs) builds and decrypts all supported
device-key/redaction combinations and verifies signatures and manifests.
It uses synthetic data and in-memory keys; no packages or keys are written to disk.

## Usage

Call `orb_pcp::build` with a `BuildRequest`, a cryptographically secure RNG and
a signer callback receiving the exact 32-byte SHA-256 digest.

- Supply encoded images, recipient keys and the shared `orb_pcp::v1` messages:
  `Info`, face embeddings, iris codes and shares, and DI embeddings and shares.
  Messages are written as given, so fields added to `pcp-defs` pass through
  without builder changes, and absent optional fields are omitted. Image
  encoding, quantization, secret sharing and key authorization belong to the
  caller.
- The builder owns every `*_salt` field and the multiframe image ID lists in
  `info.json`; caller values there are replaced. Each present salted value gets
  a fresh salt and a salted hash in `hashes.json`.
- Choose `BiometricPolicy::Included` or `Redacted`. Redaction removes the
  biometric files and their hashes and blanks all image IDs. `info.json`,
  `backend_keys.json`, the salted metadata hashes and the `backend_keys.json`
  hash remain.
- Every package is stamped 2.8: `pcp.v1` evolves additively, so new optional
  fields and files do not change the version. `device_public_key` is optional
  like any other `info` field. Tiers 1 and 2 are empty archives outside the
  manifest.
- TEE migrations set `BuildRequest::migration`; it is written as binary
  `migration.pb` and hashed in `hashes.json`.
- The builder does not verify that shares reconstruct the supplied codes or
  embeddings.

The result contains three encrypted tiers and their SHA-256 checksums.
Construction is synchronous; use a blocking worker in async applications.
Internal zeroization is best-effort; callers must manage sensitive input lifetimes.

## Local diagnostics

Enable `not-prod-diagnostics` and call `build_unencrypted_for_diagnostics` for
plaintext gzip tiers. These can contain biometrics, shares and identity metadata:
**never upload, publish or log them**. The caller must protect and clear these
buffers. Normal `build` calls remain encrypted with this feature enabled.
