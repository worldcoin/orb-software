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

- Supply encoded images, recipient keys and metadata using the shared
  `orb_pcp::v1` types. Image encoding, quantization, secret sharing and key
  authorization belong to the caller.
- Build `Biometrics` with `Biometrics::images(&images)`, optionally add `.iris(iris)`
  and `.di(di)`, then finish with `.face_embeddings(&face_embeddings)`.
- Choose `BiometricPolicy::Included(biometrics)` or `Redacted`. Redaction omits
  biometrics and clears image IDs, while retaining identity metadata.
- When supplying an Iris group, codes, all three shares, and their versions are
  required by default. Use `.strict(false)` only for compatibility with older
  PCPs missing these values. Supplied data and version consistency are still
  validated.
- For TEE migrations, supply `.migration(&migration)` before `.signer(...)`.

See the example for complete input construction and the API documentation for
validation rules and package layout.

The result contains three encrypted tiers, of which tiers 1 and 2 are empty
archives, and their SHA-256 checksums.
Construction is synchronous; use a blocking worker in async applications.
Internal zeroization is best-effort; callers must manage sensitive input lifetimes.

## Local diagnostics

Enable `not-prod-diagnostics` and call `.build_unencrypted()` instead of `.build()` for
plaintext gzip tiers. These contain biometrics, shares and identity metadata:
**never upload, publish or log them**. The caller must clear these buffers.
