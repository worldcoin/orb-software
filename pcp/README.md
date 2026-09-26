# orb-pcp

Builds Personal Custody Packages for versions 2.7, 2.8 and 3.0: payload encoding,
Hyrax commitments, archive layout, hashing, signing, compression and
alkali/libsodium sealed-box encryption.

## Build and example

Use the repository development environment, which provides the pinned Rust
toolchain, `protoc`, `pkg-config` and libsodium.

```sh
cargo build -p orb-pcp --example build_pcp
cargo run -p orb-pcp --example build_pcp
cargo test -p orb-pcp --all-features --all-targets
```

[examples/build_pcp.rs](examples/build_pcp.rs) builds and decrypts all supported
version/device-key/redaction combinations and verifies signatures and manifests.
It uses synthetic data and in-memory keys; no packages or keys are written to disk.

## Usage

Call `orb_pcp::build` with a `BuildRequest`, a cryptographically secure RNG and
a signer callback receiving the exact 32-byte SHA-256 digest.

- Supply encoded images, metadata, recipient keys, `DaugmanData`, `DiData` and
  face embeddings. Image encoding, quantization, secret sharing and key
  authorization belong to the caller.
- Choose `BiometricPolicy::Included` or `Redacted`. Redaction removes biometric
  files, hashes and image IDs, but retains identity/signup metadata.
- PCP 2.7 forbids a device key, 2.8 requires one, and 3.0 accepts either.
- Validate per-eye DI metadata agreement before grouping the inputs; the builder
  does not verify that shares reconstruct the supplied codes or embeddings.

The result contains three encrypted tiers and their SHA-256 checksums.
Construction is synchronous; use a blocking worker in async applications.
Internal zeroization is best-effort; callers must manage sensitive input lifetimes.

## Local diagnostics

Enable `not-prod-diagnostics` and call `build_unencrypted_for_diagnostics` for
plaintext gzip tiers. These can contain biometrics, shares and identity metadata:
**never upload, publish or log them**. The caller must protect and clear these
buffers. Normal `build` calls remain encrypted with this feature enabled.
