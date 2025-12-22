# orb-optee-secure-storage

A Rust implementation of an OP-TEE secure storage API. It implements a way to
easily read/write to a persistent, encrypted binary file that is addressed by a
string key. Secure storage is ultimately backed by the jetson-fuse-derived
[secure storage key](SSK), so this binary does *not* interact with the secure
element at all - it merely requires /usr/persistent/tee to not be corrupted.

It is accessible via either CLI or as a crate.

## How access controls work

The orb's threat model requires that TA code should trust neither CAs nor the
linux kernel. For this and other reasons, TAs are designed that key derivation,
encryption, decryption, and signing all happen *inside* OP-TEE in an isolated
TrustZone environment, and not export that information to linux. In addition,
*because* OP-TEE cannot trust linux at all and is isolated from it, it also has
no way to enforce access controls on CAs. It must assume that the entirety of
both the linux kernel as well as userland are compromised and therefore this
implies that the TA must assume that access controls to the TAs are entirely
unrestricted.

That being said, it is still beneficial to harden access controls to TAs in
linux. In order to communicate with a TA, a CA will [access /dev/tee][/dev/tee]
and [issue various ioctl calls][CA ioctl]. The linux kernel's TEE driver will
then [detect the effective userid][linux driver euid] of the calling process,
derive from it a new per-user UUIDv5, and report this uuid to the TA. The TA
can then read this uuid using the [`gpd.client.identity`][ClientIdentity]
property, which OP-TEE provides to all TAs once a session has been initialized.

`orb-secure-storage-ta` uses this reported linux effective user id to further
isolate the keyspace by prefixing PersistentObject IDs by the effective user id
reported by linux. Doing this ensures that that each linux user sees a
different set of keys, and enables the linux kernel to actually enforce the
access controls to TAs based on conventional linux userid semantics.

Note that as stated previously, this access control ultimately is *enforced* by
linux. As always, it is still necessary for TAs to never rely on linux for any
load-bearing security guarantees, so it is still important to separate TAs by
"use case" and not *just* the linux-reported euid. This will be facilitated in
the future by "librarifying" the TA code to make it trivial to add new TAs,
each with their own distinct UUIDs and [Trusted Application Storage Keys][TSK].
`orb-secure-storage-ca` will then support choosing *which* TA to talk to based
on a rust enum.

In the meatime, the only "use case" that uses `orb-secure-storage-ta` is
`orb-connd`'s network profile encryption.

## Comparision to wld-enc-ss

`wld-enc-ss` is a (currently closed source) TA/CA that lives in the private
[TrustZone][worldcoin/TrustZone] repo which handles our key encryption for ai
models.

`orb-secure-storage` is very similar, but with some improvements:

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

The long term goal is probably to replace wld-enc-ss, if people like it.

[optee-teec]: https://teaclave.apache.org/api-docs/trustzone-sdk/optee_teec/
[optee-utee]: https://teaclave.apache.org/api-docs/trustzone-sdk/optee_utee/
[worldcoin/TrustZone]: https://github.com/worldcoin/TrustZone
[SSK]: https://optee.readthedocs.io/en/latest/architecture/secure_storage.html#secure-storage-key-ssk
[linux driver euid]: https://elixir.bootlin.com/linux/v5.15.148/source/drivers/tee/tee_core.c#L234
[/dev/tee]: https://github.com/OP-TEE/optee_client/blob/d6c3b39db151dae1ee1f056d4f04057e56f0e0d9/libteec/src/tee_client_api.c#L172
[ClientIdentity]: https://globalplatform.org/wp-content/uploads/2021/03/GPD_TEE_Internal_Core_API_Specification_v1.3.1_PublicRelease_CC.pdf#page=87
[CA ioctl]: https://github.com/OP-TEE/optee_client/blob/d6c3b39db151dae1ee1f056d4f04057e56f0e0d9/libteec/src/tee_client_api.c#L728
[TSK]: https://optee.readthedocs.io/en/latest/architecture/secure_storage.html#trusted-application-storage-key-tsk
