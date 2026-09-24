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
including 3.0 with and without a device key, using exclusively synthetic data
and freshly generated in-memory keys. It opens
the encrypted tiers with alkali/libsodium, checks archive routing, verifies the
example's P-256 prehash signature, and checks the complete manifest against
decrypted payloads, salted metadata, and version-3 encrypted tier hashes. It only
prints version, device-key presence, redaction state, encrypted lengths and success; it writes no
packages, keys or payloads to disk. The example is also an integration test.
It includes extra captured IR images without normalized outputs, as supported
by the existing format, and checks their files, metadata IDs and manifest hashes.

P-256 is an example signer choice, not a requirement of the library. Certificates,
embeddings and shares in the example are placeholders, not a real signup or a
proof of biometric validity.

## API

Use `orb_pcp::build` with a `BuildRequest` and explicit `PcpVersion`.
`BiometricPolicy::Included { daugman, di, face_embeddings, images, ... }` supplies
separate named biometric inputs;
`BiometricPolicy::Redacted` omits them. Input types and errors are exported
directly from `orb_pcp`; archive layout and intermediate encoders are private.
See `examples/build_pcp.rs` for a complete caller using this API.

`DaugmanData` groups common pipeline/sharing versions with left and right
`DaugmanEyeData` values (codes, masks and their three recipients' shares).
`DiData` similarly groups common model/embedding/sharing versions and inference
backend with optional left and right `DiEyeData` values (quantized/float
embeddings, mirrored embeddings and shares). Shares at the same index belong
to the same recipient throughout. These input groups do not change output
filenames or wire formats.

Daugman and DI each remain grouped through encoding and archive assembly.
Versions belong to the input data and their serialized files, not duplicate
metadata alongside the encoded buffers. Iris image/normalization inputs are
separate from the Daugman code/share representation.

Before constructing `DiData` from independently produced eyes, the consumer
adapter must reject disagreements in model version, embedding version or
inference backend. The builder receives that metadata once and cannot check
discarded per-eye metadata. It does not verify share reconstruction or model
provenance. `di: None` or either missing eye emits four present, empty DI files;
two present eyes with empty vectors still emit present protobuf records.
Missing iris codes/masks/version serialize as JSON null, while iris shares
remain required.

Sealed-box encryption uses alkali's maintained binding to the same libsodium
Curve25519/XSalsa20-Poly1305 construction. Native failures, including unacceptable
recipient keys, are returned as errors. There is no sodiumoxide dependency or
advisory exception in this workspace. The native libsodium/pkg-config requirement
remains; this change does not replace the cryptographic implementation.

### Memory cleanup

Owned inner archives, tier archives, and compressed intermediate buffers use
`Zeroizing<Vec<u8>>` from allocation, including error paths. Replacing a plaintext
archive with its ciphertext drops and wipes the old buffer. Generated Hyrax
blinding-factor buffers and the local seed are also wiped on normal drop.

This is best-effort cleanup, not comprehensive secret-memory protection. The
upstream Hyrax API takes its seed by value and does not wipe that copy or all
internal state. Caller-owned inputs, serialized JSON/protobuf payload buffers,
allocator reallocations, codec-internal copies, and returned plaintext diagnostic
buffers are not all wiped. Process aborts can skip destructors. Do not interpret
the use of `Zeroizing` as a guarantee that no plaintext remains in RAM.

## Unencrypted diagnostics

For local diagnostics only, explicitly enable the `not-prod-diagnostics` feature
and call `build_unencrypted_for_diagnostics`. It returns a distinct
`DiagnosticPackage`: three gzip-compressed tiers containing plaintext inner
archives. Hashing, signing, redaction and layout still run; V3 tier hashes cover
the diagnostic gzip bytes. Recipient keys are not validated or used to encrypt.

These buffers can contain identity metadata, raw biometrics, secret shares and
Hyrax blinding factors. **Never upload them as a PCP, publish them or log them.**
The caller owns access control, secure storage and cleanup; buffers are not
automatically zeroized. The library writes no files. The normal `build` function
always encrypts, even with this feature enabled; there is no global disable switch
or conversion from `DiagnosticPackage` to `Package`.

## Scope and limitations

This is a working construction prototype, not a production cutover or an
untrusted-package verifier. Call it from a blocking worker in async applications.
The consumer owns signer/recipient authorization, signer deadlines and retries,
PNG encoding, quantization and secret sharing. Plaintext intermediates are not
all automatically zeroized.

`BiometricPolicy::Redacted` removes biometric files, their hashes and image IDs
together; it deliberately retains identity/signup metadata. `Included` accepts
the currently supported complete profile. Optional input fields anticipate
partial-data support, but the existing wire behavior is preserved until that
upstream change is finalized and separately tested.

Hyrax generation uses the pinned legacy implementation, including its empty
commitment behavior for inputs of 256 bytes or fewer. Checked import of existing
commitments is not implemented. Production integration, complete private
version/variant differential validation, and operational rollout remain separate
work.

### Hyrax dependency maintenance

Hyrax is pinned to the full Git revision
`ec5f1120e394643ad09990a34815dd11e9122366` to preserve existing commitment outputs.
Its SHAKE256 generator derivation uses `sha3 0.8` / `digest 0.8`; replacing them
requires deterministic output-parity tests, not just a successful build. These
older versions are maintenance debt, not by themselves evidence of a vulnerability.

The upstream manifest also declares `bincode 1.3.3`, although its source does not
use it and commitment serialization does not use bincode. Bincode is
[unmaintained (RUSTSEC-2025-0141)](https://rustsec.org/advisories/RUSTSEC-2025-0141.html).
Removing this unused dependency and updating SHAKE256 belong in a reviewed
upstream change; upgrading the pin alone does not resolve them as of September
2026. No advisory exception is added here. The workspace's existing policy fails
on unmaintained direct dependencies, not transitive ones; a passing check does
not establish that every transitive dependency is maintained.
