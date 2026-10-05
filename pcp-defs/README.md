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
set and ignore fields they don't know. That is how a single `pcp.v1` covers
both 2.7 and 2.8.

`hashes.sign` covers the original `hashes.json` bytes. Verify against those
bytes, never against a re-encoded message.

## PCP 2.9 migration artifacts (proposal)

- `migration.pb` (binary protobuf `Migration`): TEE software and biometric
  pipeline versions, source signup ID and PCP version, and migration timestamp.
  `migrated_ts` is uint64 Unix seconds.
- `legacy/`: preserved source biometrics and original manifest/signature, as
  defined below.
- `Migration.src_signup_id`: the source package's `Info.signup_id`.
  The new package's `Info.signup_id` identifies the new signup.
- `Hashes.migration_pb` (tag 59): SHA-256 of the complete `migration.pb` file.
  Hash and verify the exact emitted `migration.pb` bytes,
  never a decoded and re-encoded message.

These additions are optional for ordinary captures. The migration builder must
require `migration.pb` and the legacy contents below.

### Legacy directory contract

`legacy/` is a directory inside the package tar, under the same outer user
encryption as the current biometric artifacts. Preserve each source file's
original basename and exact bytes, including JSON whitespace and protobuf
encoding. Fresh biometric outputs retain their existing top-level paths.

| Package path | Contents | Presence |
| --- | --- | --- |
| `legacy/iris_codes.json` | Original iris codes and masks | When present in the source |
| `legacy/iris_code_shares_{0,1,2}.json` | Original iris code shares | Each file present in the source |
| `legacy/di_iris_embeddings.pb` | Original DI embeddings | When present in the source |
| `legacy/di_iris_embeddings_shares_{0,1,2}.pb` | Original DI embedding shares | Each file present in the source |
| `legacy/face_embeddings.json` | Original face embeddings | When present in the source |
| `legacy/hashes.json` | Complete original signed manifest | Required |
| `legacy/hashes.sign` | Original manifest signature | Required |

Preserve every available source artifact listed above.
This directory contains only the listed artifacts; capture metadata,
key envelopes, raw images and other archives are outside this legacy contract.

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
