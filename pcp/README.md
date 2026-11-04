# orb-pcp

Builds Personal Custody Packages: payload encoding, Hyrax commitments, archive
layout, hashing, signing, compression and alkali/libsodium sealed-box encryption.

## Build and example

Use the repository development environment, which provides the pinned Rust
toolchain, `protoc`, `pkg-config` and libsodium.

```sh
cargo build -p orb-pcp --example build_pcp
cargo run -p orb-pcp --example build_pcp
cargo test -p orb-pcp --all-features --all-targets
```

[examples/build_pcp.rs](examples/build_pcp.rs) builds synthetic packages,
included and redacted, then decrypts them and verifies their contents, signatures
and manifests. Keys and packages stay in memory.

## Usage

Start with `orb_pcp::Builder::timestamp`, supply the inputs and a cryptographically
secure RNG, then finish configuration with `.signer(callback)`. Call `.build()`
on the resulting builder. The callback receives the exact 32-byte SHA-256 digest.

- Supply encoded images, recipient keys and the shared `orb_pcp::v1` messages:
  `Info`, face embeddings, iris codes and shares, and DI embeddings and shares.
  Accepted messages are written as given; absent optional JSON fields are omitted.
  Image encoding, quantization, secret sharing and key authorization belong to
  the caller.
- The builder owns every `*_salt` field and the multiframe image ID lists in
  `info.json`; caller values there are replaced. Each present salted value gets
  a fresh salt and a salted hash in `hashes.json`.
- Multiframe captures are additional IR frames of an eye. Their `ImageId`s name
  their files (`{image_id}.png` and, with per-frame normalization,
  `{image_id}_normalized_*.bin`) and fill the multiframe ID lists.
- Choose `BiometricPolicy::Included(biometrics)` or `Redacted`. Redaction removes the
  biometric files and their hashes and blanks all image IDs. `info.json`,
  `backend_keys.json`, the salted metadata hashes and the `backend_keys.json`
  hash remain.
- TEE migrations supply `.migration(&migration)` before `.signer(...)`; it is written as binary
  `migration.pb` and hashed in `hashes.json`.
- Build `Biometrics` with `Biometrics::images(&images)`, optionally supply
  `.iris(iris)` and `.di(di)`, then finish with `.face_embeddings(&face_embeddings)`.
  The two groups are independent and may both be included. An omitted group
  contributes no files or hashes; an included group contributes its data file
  and all three share files. Share array indices identify recipients.
- Construct each group with `Iris::codes(&codes).shares(shares)` or
  `Di::embeddings(&embeddings).shares(shares)`. Both take three share references
  and return `BiometricValidationError` when required fields are absent.
  `Iris` checks that code, mask and version fields are present.
  `Di` checks that the embedding record and each share record are present.
  These checks do not validate numeric ranges or version consistency.
- Every manifest is stamped `orb_pcp::PCP_VERSION`.

The result contains three encrypted tiers, of which tiers 1 and 2 are empty
archives, and their SHA-256 checksums.
Construction is synchronous; use a blocking worker in async applications.
Internal zeroization is best-effort; callers must manage sensitive input lifetimes.

## Local diagnostics

Enable `not-prod-diagnostics` and call `.build_unencrypted()` instead of `.build()` for
plaintext gzip tiers. These contain biometrics, shares and identity metadata:
**never upload, publish or log them**. The caller must clear these buffers.
