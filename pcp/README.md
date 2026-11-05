# orb-pcp

Builds Personal Custody Packages: payload encoding, Hyrax commitments, archive
layout, hashing, signing, compression and caller-supplied sealed-box encryption.

## Build and example

Use the repository development environment, which provides the pinned Rust
toolchain and `protoc`. Tests and the example also use `pkg-config` and libsodium
through the Alkali dev-dependency; library consumers choose their own binding.

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
secure RNG, set `.sealer(seal)`, then finish configuration with `.signer(callback)`.
Call `.build()` on the resulting builder. The signer receives the exact 32-byte
SHA-256 digest.

The sealer has signature
`fn(&[u8], &[u8; 32]) -> Result<Vec<u8>, SealError>`, with a caller-defined error
type. It must produce libsodium-compatible sealed boxes (Curve25519,
XSalsa20-Poly1305, and BLAKE2b nonce derivation), preserve the exact recipient
bytes, use fresh ephemeral keys, and include the standard 48-byte overhead.
The caller initializes its crypto library and propagates the failures it exposes.
Sealing errors are returned as `BuildError::Encryption`.

The example supplies the Alkali adapter in [tests/support/mod.rs](tests/support/mod.rs).
Consumers using Alkali can copy that adapter and its concrete error type into their
own crate; `orb-pcp` has no normal dependency on Alkali.

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
- Supply optional ORBIT5 bundles with
  `Di::embeddings(&embeddings).orbit5_di_embedding_shares(&bundles).shares(shares)`.
  Omitting the ORBIT5 input omits that entire family; supplying it writes all five
  `orbit5_di_iris_embeddings_shares_{0..4}.pb` files and their signed hashes,
  including zero-byte files for default messages. Omitting the DI group or
  redacting biometrics also omits the family. The caller owns share generation
  and validation of ORBIT5 dimensions, values and metadata.

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

The builder still requires `.sealer(...)` for diagnostics, but
`.build_unencrypted()` never calls it.
