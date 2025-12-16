# orb-optee-secure-storage

A Rust implementation of an OP-TEE secure storage API. It implements a way to
easily read/write to a persistent, encrypted binary file that is addressed by a
string key. Secure storage is ultimately backed by the jetson-fuse-derived
[secure storage key](SSK), so this binary does *not* interact with the secure element
at all - it merely requires /usr/persistent/tee to not be corrupted.

It is accessible via either CLI or as a crate.

## Motivation

Good security practices dictate that access to CAs should be as narrow/limited as possible.
However, the overhead of creating a new CA for every use case is rather high. Instead
of creating many CAs for the same duplicated functionality, we can use a *single* CA
and scope access by user-id of the calling process, so that different user ids have
different key namespaces.

It was also written as a learning exercise by @thebutlah to understand how trustzone
works.

## Comparision to wld-enc-ss

`wld-enc-ss` is a (currently closed source) TA/CA that lives in the private
[TrustZone][worldcoin/TrustZone] repo which handles our key encryption for ai models.

`orb-optee-secure-storage` is very similar, but with some improvements:

* 🦀 Its Rust, not C.
  * Proper docs.rs for trustzone [CA](optee_teec) and [TA](optee-utee) apis.
  * Package management via cargo (even in no-std TAs!).
  * More security hardened than C.
  * (Debatably) easier to contribute to.
* Its built in orb-software so its FOSS.
* Its decoupled from the build system of the trusted-os itself, so its build system is
  simpler to understand and faster.
* It scopes storage by the UID that invoked the program, ensuring that different users
  cannot read or write to each others' storage.
* It accepts any arbitrary string key as key instead of a finite set of keys.

The long term goal is probably to replace wld-enc-ss, if people like it, since it is
(hopefully) more secure.

[optee-teec]: https://teaclave.apache.org/api-docs/trustzone-sdk/optee_teec/
[optee-utee]: https://teaclave.apache.org/api-docs/trustzone-sdk/optee_utee/
[worldcoin/TrustZone]: https://github.com/worldcoin/TrustZone
[SSK]: https://optee.readthedocs.io/en/latest/architecture/secure_storage.html#secure-storage-key-ssk
