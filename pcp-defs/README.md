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

- `migration.json` (`Migration`): TEE/pipeline versions, source signup/version/manifest
  digest, migration timestamp and optional enclave measurement. `migrated_ts` is
  uint64 Unix seconds, serialized as a decimal string in ProtoJSON.
- `legacy.tar`: preserved source artifacts, with byte-identical contents.
- `Info.src_signup_id` (tag 29): source `Info.signup_id`, matching
  `Migration.src_signup_id`; `Info.signup_id` identifies the new signup.
- `Hashes` tags 59–61: SHA-256 of the complete `migration.json`, `legacy.tar` and
  `info.json` files.

These additions are optional for ordinary captures. The migration builder must
require both artifacts and matching source IDs.

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
