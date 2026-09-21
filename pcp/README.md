# orb-pcp prototype

Portable, synchronous Personal Custody Package construction for versions 2.7,
2.8 and 3.0. Consumers provide encoded images, metadata, embeddings/shares,
recipient public keys, a cryptographically secure random source, and a callback
that signs the exact raw SHA-256 digest. The crate generates legacy-compatible
Hyrax commitments and assembles, hashes, signs, compresses and encrypts the tiers.

## Build and run

Use the repository development environment. Building requires the pinned Rust
toolchain, `protoc`, `pkg-config`, and the libsodium development package accessible
through `pkg-config` (`libsodium.pc`).

```sh
cargo build -p orb-pcp --example build_pcp
cargo run -p orb-pcp --example build_pcp
cargo test -p orb-pcp --test prototype
```

The example builds all three versions with included and redacted biometrics,
using exclusively synthetic data and freshly generated in-memory keys. It opens
the encrypted tiers with sodiumoxide, checks archive routing, verifies the
example's P-256 prehash signature, and checks the complete manifest against
decrypted payloads, salted metadata, and version-3 encrypted tier hashes. It only
prints version, redaction state, encrypted lengths and success; it writes no
packages, keys or payloads to disk. The example is also an integration test.

P-256 is an example signer choice, not a requirement of the library. Certificates,
embeddings and shares in the example are placeholders, not a real signup or a
proof of biometric validity.

## Scope and limitations

This is a working construction prototype, not a production cutover or an
untrusted-package verifier. Call it from a blocking worker in async applications.
The consumer owns signer/recipient authorization, signer deadlines and retries,
PNG encoding, quantization and secret sharing. Plaintext intermediates are not
all automatically zeroized.

`Biometrics::Redacted` removes biometric files, their hashes and image IDs
together; it deliberately retains identity/signup metadata. `Included` accepts
the currently supported complete profile. Optional input fields anticipate
partial-data support, but the existing wire behavior is preserved until that
upstream change is finalized and separately tested.

Hyrax generation uses the pinned legacy implementation, including its empty
commitment behavior for inputs of 256 bytes or fewer. Checked import of existing
commitments is not implemented. Production integration, complete private
version/variant differential validation, and operational rollout remain separate
work.
