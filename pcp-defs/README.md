# pcp-defs

Protobuf definitions for PCP (Personal Custody Package) messages.

## Versioning

The proto package version (`pcp.v1`) is not the PCP version. The PCP version
travels in the data itself.

Bump the package (`pcp.v2`) only for breaking changes: removing, renaming or
renumbering a field, or changing its type.

Adding a field is not a new package version. New fields are `optional` or
`repeated`, and consumers (oxide, backends) must treat an absent field as not
set and ignore fields they don't know. That is how a single `pcp.v1` covers
both 2.7 and 2.8.

`hashes.sign` covers the original `hashes.json` bytes. Verify against those
bytes, never against a re-encoded message.
