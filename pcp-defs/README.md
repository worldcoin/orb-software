# pcp-defs

Protobuf definitions for PCP (Personal Custody Package) messages.

## Versioning

The proto package version (`pcp.v1`) is not the PCP version. The PCP version
travels in the data itself.

Bump the package (`pcp.v2`) only for breaking changes: renaming or
renumbering a field, or changing its type. A removed field keeps its
number and name as `reserved` so neither is ever reused.

Adding a field is not a new package version. New fields are `optional` or
`repeated`, and consumers (oxide, backends) must treat an absent field as not
set and ignore fields they don't know. A single `pcp.v1` can cover multiple
PCP versions.

`hashes.sign` covers the original `hashes.json` bytes. Verify against those
bytes, never against a re-encoded message.

## Migration artifacts

- `migration.pb` (binary protobuf `Migration`): TEE software and biometric
  pipeline versions, source signup ID and PCP version, migration timestamp,
  and manifest-signing key ID.
  `migrated_ts` is uint64 Unix seconds.
- `Migration.signing_key_id`: the identifier of the key used by the TEE to sign
  the migrated PCP manifest. Absent in packages generated before this field was added.
- `Migration.src_signup_id`: the source package's `Info.signup_id`.
  The new package's `Info.signup_id` identifies the new signup.
- `Hashes.migration_pb` (tag 59): SHA-256 of the complete `migration.pb` file.
  Hash and verify the exact emitted `migration.pb` bytes,
  never a decoded and re-encoded message.

These additions are optional for ordinary captures. The migration builder must
require `migration.pb` and its hash in `hashes.json`.

The TEE verifies the source manifest and signature before processing the
package, then produces a new `hashes.json` and `hashes.sign` covering the
migrated package.

## Five-party DI shares

`di_iris_embeddings_5_party_shares_0.pb` through `_4.pb` encode
`DiIrisEmbedding5PartyShares`. Each file contains one recipient's left, left
mirrored, right and right mirrored vectors: 512 unsigned 16-bit values per
vector. Filename indices 0–4 correspond to Shamir IDs 1–5.

Producers record the sharing implementation revision in `shares_version`.
Consumers must validate vector dimensions and value bounds before using the
shares and agree on the sharing scheme and recipient order. The message name
does not distinguish protobuf wire encodings.

The five optional `Hashes` fields use tags 61–65; tag 60 belongs to
`reference_image.pb`. Producers must include all five bundles and hashes when
five-party sharing is enabled, alongside the existing three-party DI family, and
omit them under biometric redaction. The PCP version remains 2.8. Producers must
enable five-party sharing only for consumers that support this file family and
its extended manifest.

## Next breaking version

Changes that need a new package because they change the signed bytes:

- Nest per-frame hashes under their own map. Today they sit as flat keys in
  `hashes.json`, so no message can describe them and readers have to ignore
  unknown keys.
- Write and sign the manifest as protobuf, not JSON, so a ZK proof does not
  have to parse JSON.
- Move the remaining JSON files (iris codes, iris code shares, face
  embeddings, info, backend keys) to `.pb`, the way the deep-identifier files
  moved in 2.7. Iris code shares can then share a layout with the
  deep-identifier shares, still one file per share.
- Carry binary payloads as `bytes`, not base64 or hex strings.
- Model each salted value as one `{value, salt}` type, not two sibling
  entries.

## Checking a real package

`cargo run -p orb-pcp-defs --example check_tier0 -- <tier0.tar.gz>`
checks an unencrypted tier0 exported by orb-core (`not-prod-pcp-export` and
`not-prod-pcp-no-encrypt`):

- every JSON and `.pb` file decodes into its `v1` type and re-encodes to the
  same bytes. Decoding ignores unknown fields, so this is what catches a key
  the orb writes but the protos lack.
- `hashes.json` has a matching digest for every file, and for every salted
  `info.json` field.

The packages hold raw biometrics, so keep them out of git.
