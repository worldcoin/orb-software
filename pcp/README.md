# orb-pcp

Builds Personal Custody Packages for versions 2.7, 2.8, 2.9 and 3.0: payload encoding,
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
- PCP 2.7 forbids a device key, 2.8 and 2.9 require one, and 3.0 accepts either.
  A 2.9 migration may omit it.
- Validate per-eye DI metadata agreement before grouping the inputs; the builder
  does not verify that shares reconstruct the supplied codes or embeddings.

## PCP 2.9 migrations

PCP 2.9 uses the 2.8 layout and is produced both on the Orb and in the TEE.
Orb captures leave `BuildRequest::migration` as `None` and follow the 2.8 rules.
TEE migrations of older packages set it; other versions reject it.

- `MigrationProvenance` becomes binary `migration.pb`, hashed in `hashes.json`
  as `migration.pb`. Its `src_signup_id` is also written unsalted to `info.json`.
- `LegacyArtifacts` holds the exact source bytes for the fixed `legacy/` file
  set. The source `hashes.json` and `hashes.sign` are required; other files are
  written only when supplied. Legacy files are not covered by the new manifest.
- A migration requires included biometrics. Because its source may predate
  them, it may omit the device key, optional `PackageInfo` fields, primary iris
  image IDs and the thumbnail ID. An absent field is omitted together with its
  salt and hash.

The result contains three encrypted tiers and their SHA-256 checksums.
Construction is synchronous; use a blocking worker in async applications.
Internal zeroization is best-effort; callers must manage sensitive input lifetimes.

## Local diagnostics

Enable `not-prod-diagnostics` and call `build_unencrypted_for_diagnostics` for
plaintext gzip tiers. These can contain biometrics, shares and identity metadata:
**never upload, publish or log them**. The caller must protect and clear these
buffers. Normal `build` calls remain encrypted with this feature enabled.
