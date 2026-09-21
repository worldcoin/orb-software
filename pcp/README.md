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
It includes extra captured IR images without normalized outputs, as supported
by the existing format, and checks their files, metadata IDs and manifest hashes.

P-256 is an example signer choice, not a requirement of the library. Certificates,
embeddings and shares in the example are placeholders, not a real signup or a
proof of biometric validity.

## API

Use `orb_pcp::build` with a `BuildRequest` and explicit `PcpVersion`.
`BiometricPolicy::Included { iris, di, face_embeddings, images, ... }` supplies
separate named biometric inputs;
`BiometricPolicy::Redacted` omits them. Input types and errors are exported
directly from `orb_pcp`; archive layout and intermediate encoders are private.
See `examples/build_pcp.rs` for a complete caller using this API.

`IrisData` groups common pipeline/sharing versions with left and right
`IrisEyeData` values (codes, masks and their three recipients' shares).
`DiData` similarly groups common model/embedding/sharing versions and inference
backend with optional left and right `DiEyeData` values (quantized/float
embeddings, mirrored embeddings and shares). Shares at the same index belong
to the same recipient throughout. These input groups do not change output
filenames or wire formats.

Before constructing `DiData` from independently produced eyes, the consumer
adapter must reject disagreements in model version, embedding version or
inference backend. The builder receives that metadata once and cannot check
discarded per-eye metadata. It does not verify share reconstruction or model
provenance. `di: None` or either missing eye emits four present, empty DI files;
two present eyes with empty vectors still emit present protobuf records.
Missing iris codes/masks/version serialize as JSON null, while iris shares
remain required.

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
