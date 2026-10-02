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

`cargo run -p orb-pcp-defs --example check_tier0 -- <tier0.tar.gz> [other]`
checks an unencrypted tier0 exported by orb-core (`not-prod-pcp-export` and
`not-prod-pcp-no-encrypt`):

- every JSON and `.pb` file decodes into its `v1` type and re-encodes to the
  same bytes. Decoding ignores unknown fields, so this is what catches a key
  the orb writes but the protos lack.
- `hashes.json` has a matching digest for every file, and for every salted
  `info.json` field.
- given a second package, each JSON file has the same keys and value types,
  which shows whether two orb-core builds write the same layout.

The packages hold raw biometrics, so keep them out of git.
