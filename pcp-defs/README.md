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

PCP 2.9 keeps the v2.8 JSON/protobuf artifact encodings and tier placement. A
migration adds two optional tier-0 artifacts:

| Artifact | Representation | Manifest entry |
| --- | --- | --- |
| `migration.json` | `pcp.v1.Migration`, serialized as JSON | `Hashes.migration_json`, JSON key `migration.json` |
| `legacy.tar` | Opaque tar containing original source artifact bytes | `Hashes.legacy_tar`, JSON key `legacy.tar` |

`migration.json` records the migration execution. Original Orb capture fields
remain in `info.json`. Its new optional `src_signup_id` field (tag 29) records the
source package's `Info.signup_id`; the active `signup_id` is the new signup.
Ordinary captures omit `src_signup_id`. For a migration, `Info.src_signup_id`
and `Migration.src_signup_id` must both equal the source's `Info.signup_id`.
The whole-document `info.json` digest covers this field; no separate salt or
salted-field hash is added.

Both artifacts are absent for ordinary Orb captures. For a successful TEE
migration, the proposed builder profile requires both, together with their
manifest digests. Optional protobuf fields express absence and compatibility;
they do not enforce this artifact-level rule or validate successful inference.

### Migration metadata

| Field | Meaning |
| --- | --- |
| `tee_version` | Release version of the executing `di-migration-tee` software, analogous to `Info.software_version` |
| `src_signup_id` | Source package's `Info.signup_id` |
| `source_pcp_version` | Source PCP version |
| `source_hashes_sha256` | SHA-256 of the original `hashes.json` bytes |
| `migrated_ts` | Migration timestamp in Unix seconds (`uint64`) |
| `enclave_measurement` | Optional enclave measurement |
| `biometric_pipeline_version` | Release version of the executed biometric pipeline |

All fields use explicit presence, following the existing capture schemas, and
snake-case JSON names. Standard ProtoJSON writes `migrated_ts` as a decimal
string and accepts both string and numeric input. `Info.timestamp` remains the
original capture time. Individual model versions and inference backends remain
in their biometric artifacts; no nested pipeline metadata is added here.

The producer must validate source identity, execution versions, timestamps and
64-character lowercase SHA-256 hex digests. No capture metadata is backfilled
from execution settings.

### Preserved originals

`legacy.tar` is deliberately not a protobuf message or a re-encoding of source
JSON. Its relative member names are:

- `info.json`, `hashes.json`, `hashes.sign`;
- `backend_keys.json`, `iris_codes.json`, `face_embeddings.json`, and each
  `iris_code_shares_{0,1,2}.json`, when present in the source;
- an opaque source `face_ir_and_thermal.tar`, when present.

Preserve member bytes exactly, including unknown legacy JSON fields, whitespace,
old face head pose and absent/null distinctions. Do not invent missing members.
Original raw captures remain in the existing modality archives. Old normalized
images/commitments and DI derivatives are replaced by the current execution.
This is a selected set of originals, not a complete backup of the source PCP.
Keeping its manifest/signature does not recover discarded artifacts or establish
that historically unsigned metadata was authenticated.

The unique outer `legacy.tar` name avoids basename collisions with active files
such as `iris_codes.json` in the flat manifest. Hash the exact complete tar bytes
once; do not flatten its member names into the new manifest. It remains inside
the user-encrypted tier 0, and does not authorize separate export of biometrics.

### Integrity and integration

The three new `Hashes` fields are optional hex SHA-256 strings, appended at tags
59–61 without changing existing tags or JSON names. In addition to existing
artifact/salted-field coverage, the proposed v2.9 builder hashes the **whole**
`info.json`, `migration.json` and `legacy.tar`. Whole `info.json` coverage binds
capture fields previously absent from the salted-field manifest entries.
Sign the final exact manifest bytes with the enclave signer; the preserved Orb
certificate is historical data, not the new signing authority.

After assembly, the TEE compares retained raw captures and legacy members
byte-for-byte with the authenticated opened source and rejects missing, changed
or unexpected retained data before returning the package. Fresh results,
identity changes and regenerated salts follow separate validation rules.

This change defines schemas only. The builder's V2_9 profile, tar assembly,
manifest construction, preservation gate, and client trust/verification support
remain integration work. The shared builder extraction is tracked in
[PR #1419](https://github.com/worldcoin/orb-software/pull/1419); these definitions
extend [PR #1437](https://github.com/worldcoin/orb-software/pull/1437).
Older schemas may ignore the additions when decoding, but that does not make an
older client a valid v2.9 verifier. This remains an additive `pcp.v1` change.

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
