# Changelog

## 6.0.0

### Breaking changes:

+ Requires a change to the config file format. Instead of `pubkey = "/config/pubkey"`
  we use `verify_manifest_signature_against = "prod"`. See
  [here](https://github.com/worldcoin/orb-update-agent/blob/5aa13949d4a3666c0c8ba9b6d7506596e129042f/update-agent/src/settings/tests.rs#L286-L300)
  as an example. The two supported values are "prod" and "stage".
+ Packaging now is done via cargo deb. Artifacts produced in CI are therefore now
  different.

### Added

+ Re-implemented the manifest signature verification feature. It now vendors the pubkeys
  and uses JWK as the key format. The check is skipped if you build with
  `--feature skip-manifest-signature-verification`.

### Changed

+ Changed how version numbers are reported. We now use orb-software's orb-build-info
  crate.
+ Version of the update-agent-core crate is now 0.0.0, like all our other libraries.
+ Prefixed both crates with an `orb-` prefix, just like all our other crates.
+ [Increased timeouts for MCU ack](https://github.com/worldcoin/orb-update-agent/pull/370).

## 5.3.7

### Fixed

+ Don't fail on reading the OsIndications EFI, continue with setting the next slot.

## 5.3.6

### Fixed

+ Only set the next slot if the OsIndications EFI variable indicates a capsule update is scheduled.
+ Write capsules to `EFI/UpdateCapsule` instead of `EFI/UpdateCapsule`

## 5.3.5

### Changed

+ Removed auth token from /check request for claim

## 5.3.4

### Changed

+ Removed backend certificate pinning to avoid circumstances where an Orb falls out of sync with the root CA's
  certificates and is unable to communicate with the update backend after an extended period of going without updates.

### Fixed

+ Log download progress as percentage of total file size _remaining_, after subtracting number of bytes already downloaded
  if any.

## 5.3.3

### Fixed

+ Fixed divide-by-zero issue when component was smaller than single download chunk size

## 5.3.2

### Fixed

+ Adding logging of the update progress to journald.

## 5.3.1

### Fixed

+ Filter logs when writing to journald to INFO-level and higher (e.g. WARNING, ERROR)

## 5.3.0

### Fixed

+ Shutdown instead of reboot when finalizing updates. We are committing to always updating the MCU so as to keep
  the MCU slots in alignment with the Jetson slots. Since the MCU cannot detect a reboot, we must rely on the MCU
  to restart the Jetson after a shutdown.

## 5.2.2

### Fixed

+ Remove caching of all `orb-supervisor` DBus properties. Previously, a stale cache of the `orb-supervisor`'s
  `BackgroundDownloadsAllowed` property would prevent the `update-agent` from returning to an unthrottled speed
  in the case where it started with the `BackgroundDownloadsAllowed` being `true` and then had the value change
  to `false`.

## 5.2.1

### Fixed

+ Subtract the already-downloaded files from the calculated total update size before checking if the update fits
  within available disk space.

## 5.2.0

### Changed

+ **Reverted signature verification**

## 5.1.0

### Added

+ **Support for EFI capsule updates. Enables updates to device's boot chain.**
+ **Signature verification of the update claim's manifest**
+ Removal of stale downloads and disk space checks to ensure adequate space is available before downloading update

### Changed

+ Download progress is reported in human-readable percentages

### Fixed

+ Treat "RANGE" failures (when received while retrying a DFU payload) as indication of potential success. Either due
  to timeouts or bus congestion, ack messages were sometimes not received by the Jetson. The update would then fail
  during when the Jetson retried sending the DFU payload; the MCU would report that it had already processed a DFU
  payload for the range.
+ Require TLS connections on all server connections. Resolves an issue flagged during a security audit.
    + Could not be fully migrated to TLS v1.3 until AWS makes good on its commitment to support TLS v1.3 [1]
+ Dropped OpenSSL as a dependency

[1] https://aws.amazon.com/blogs/security/faster-aws-cloud-connections-with-tls-1-3/

## 5.0.2

### Fixed

+ Drain the ack receiving queue before attempting to re-send+process a DFU payload. This issue became apparent
  with the addition of retries. Acks received after the `recv_timeout` exited would still be enqueued, resulting
  in an issue where we would incorrectly assume the DFU did not work, attempt to resend the DFU, immiately process
  the ack from the previous attempt which waited at the beginning of the queue, and exit due to ack mismatch.

### Changed

+ Processing DFU acks now looks for any messages sent during a 400ms period to find an ack number match. Before,
  it only processed the first received message.
  + In combination with retries, this should improve stability w/ multiple Linux processes communicating on the bus.

## 5.0.1

### Fixed

+ Agent properly increments ack counter after sending message. Before, agent would serialize the message,
  then increment the ack counter, then send the ack, resulting in a guaranteed ack mismatch and preventing
  any mcu/CAN updates

## 5.0.0

### Added

+ `Agent` uses `slot-ctrl` library to set the next boot slot on finalizing a normal update.
+ `Agent` now follows the fallback system on II by using `slot-ctrl` and
  * setting the rootfs status `UpdateInProcess` right before writing components
  * setting the rootfs status `UpdateDone` on finalizing a normal update
  * resetting the boot retry counter of the target slot in finalizing a normal update

## 4.3.0

### Added

+ `Agent` received a `--recovery` which allows it install components with `installation_phase: recovery`
  set in their manifest entry. This flag is intended to be used while in recovery but not during
  normal operation outside of it. Note there are no safeguards here, so use with care. This flag can
  also be set in config with `recovery = true` or via the `ORB_UPDATE_AGENT_RECOVERY=true` env var.
+ `Agent` invocations are significantly sped up by doing less work (this reducing the time spent in recovery
  for full updates): after it successfully downloads and hashes an update component, it creates an empty file
  `{name}-{hash}.verified`. On a subsequent run, it will skip expensive hashing if this file is found.
  Similarly, if decompression + hashing of the downloaded was successful, a file `{name}-{hash}.uncompressed.verified`
  is created and future decompression + hashing of the matching components is skipped.

### Fixed

+ `Agent` now writes the raw server claim to disk to allow for forward-compatible. Before, `agent`
  first read the claim into a rust memory representation before serializing and writing to disk,
  potentially dropping unknown fields that would be understood by a future `agent.`

### Changed

+ Merged `loose` and `strict` settings - there is no more distinction between settings
  set through env vars, the config file, or command line arguments. Replaced config.rs by figment.
  All of these changes significantly simplify the code.
  + Note that boolean config options set through environment variables like `ORB_UPDATE_AGENT_NODBUS`
    now take the values `true` or `false`; `1` or `0` are no longer valid. This is technically a
    breaking change, but we know that the env vars were not set everywhere so we consider this
    acceptable to only bump the minor.
+ `agent` now uses CAN ISO-TP to send update blocks to the main and security microcontrollers.
  This means that we should no longer encounter ack mismatches when several services are using
  the CAN bus during an update.
+ bump `update-agent` to use `update-agent-core-0.5.1`, `update-agent-can-0.2.2`

## 4.2.1

### Fixed

+ When throttling update downloads, the agent was always sleeping for the configured maximum
  delay duration. It now sleeps for 0ms (i.e. no sleep) when the supervisor permits it.

## 4.2.0

### Changed

+ Minimal adjustments to report the error when an update request failed or was blocked.

### Fixed

+ Agent exited earlier with an error because assumptions around component versioning no
  longer hold, and because of bugs in translating versions to a flat map. Fixed by updating
  to [`update-agent-core-0.5.0`](../update-agent-core).

## 4.1.0

### Added

+ Agent shuts down the orb after an update. Operators no longer have to guess when the update is complete.

### Changed

+ Agent has a per-component timeout of 120s (30s before).
+ Agent now emits a log before hashing downloaded components so that
  logs don't seem stalled.
+ The new `versions.map` will not be preferred over the old `versions.json`. If there is a mismatch, the
  `versions.json` will be used. The new `versions.map` introduced in `4.0.0` created confusion among devs
  trying to fake the current release to trigger an update. We will eventually transition to `versions.map`,
  but for now its use is limited and so we won't take it as the single source of truth.

### Fixed

+ Agent could no longer read messages form the security MCU because its ID on the CAN bus changed.
  It can do it again.
+ Version checking of single (non-redundant) components failed if a component was updated but
  whole updating process failed (because it was no longer at the expected old version). Agent is now satisified if the single component is either at
  the original expected version or at the target version.
+ Agent no longer cleans up old manifests and components because the feature did not work as
  expected and was incomplete (for example, decompressed components were left on disk).
  This is left to tools outside the agent.
+ Agent would fail if its download directory was missing. It now creates it if missing.
+ Agent was trying to read the directory the manifest was contained in, rather than its actual path.
  It now reads the manifest path.
+ All dependencies are now either worldcoin owned or public on crates.io.

## 4.0.0

### Added

+ Agent now throttles downloading component chunks if supervisor reports that
  the orb is in use. Before, component downloading was aborted. Throttling is
  implemented by waiting `download-delay` milliseconds between downloading
  before downloading the next 4MiB chunk of a component. To set the delay to
  5000 ms, pass `--download-delay=5000` to the binary, set the environment
  variable `ORB_UPDATE_AGENT_DOWNLOAD_DELAY=5000` or set `download_delay =
  5000` in the agent's config.
+ Agent now understands `normal` and `full` updates and takes different
  actions. With `normal` updates, the agent now executes `nvbootctrl
  set-active-boot-slot` itself (before, this was done through a systemd unit).
  With `full` updates currently no action is taken. `update-agent` assumes that
  the components installed through the OTA will trigger a reboot into recovery
  by overwriting the relevant (raw) partitions. The type of update is
  configured by setting `.manifest.type: normal | full` in the update claim.
+ Agent now understands that update components are either installed during
  `normal` operation or in `recovery`, depending on their `installation_phase`
  value. This is configured through the:e
  `.manifest.components[].installation_phase` key of each component. `recovery`
  is entered by specifying the type of update through `.manifest.type`.
+ The agent is now pinned to google or AWS root certificates at compile time,
  either `Amazon_Root_CA_1` or `GTS_ROOT_R1`. Before update agent could take
  any certificate, allowing for various kinds of attacks.
+ Introduce a flat map to track versions. The agent does not currently rely on
  this but will in the future.
+ Improve reporting when trying to deserialize json, pointing to which field
  failed using `serde_path_to_error`.

### Changed

+ The update claim is now written to disk if retrieved remotely. Before, only
  the manifest was written. This is relevant for prefetching updates so that in
  full updates the recovery boot does not have to make a connection to the
  update endpoint.
+ Loosen version verification between claim and versions recorded on disk. If
  components are not present in the on-disk `versions.json` then their version
  will not be checked against the manifest. This is important because updates
  now come with new components.
+ The library is now using the upstream protobuf create from
  `github.com/worldcoin/protobuf-definitions` rather than a submodule and
  generating the code itself.

### Fixed

+ Agent reports the status code and response body if the backend responded with
  an error response. Before, the agent tried to always deserialize the error
  body as an update claim, which would always fail and led to confusing error
  messages.

## 3.0.1

### Bugs

+ Agent was not calculating the offset of single (non-redundant) raw component correctly. It
  assumed that all raw components are slotted, so it would write to `offset` when updating slot
  A, and `offset + size` when updating slot B. It will now always write the componen to `offset`
  if it is single.

### Hotfix

+ Agent was writing single GPT components even if they were mounted. This was observed to corrupt
  the SSD in some cases when the partition was still in use during the write. It first issues
  `umount -l` for single GPT components now. While this might break other services, this is deemed
  acceptable because after the update an immediate system reboot is expected. *NOTE:* this fix will
  be replaced in a future release by a proper solution.

## 3.0.0

### Bugs

+ Agent was erroring if the list of system components contained a new component
  that however was neither updated or listed in the on-disk versions.json. Now
  a warning is printed.
+ Agent was checking if every single system component was listed in the update
  manifest. Now it only checks if the actually present entries in the manifest are
  contained in system components.
+ Determining the mime type of downloaded components was not reliably resolving to
  `application/x-xz` even though `file --mime` gave the correct result. For now,
  all downloaded components are hardcoded to be xzip compressed.
+ Errors while deserializing the update claim were thrown away. Now they are
  explicitly formatted into a (somwhat ugly) error message and reported.

## 2.0.0

+ expect a list of system components as part of a new update claim
+ remove the `--components` command line argument and setting; `update-agent`
  only reads the list of system components through an update claim;
+ for each component update, expect a sh256 hash, size and mime-type in
  addition to the URL of the downloadable blob as part of the claim; the hash
  and size are different from those recorded in the update manifest, which are
  for the final decompressed component binary;
+ `update-agent` will check the hash of the downloaded component to verify its
  integrity and before decompressing it; if the integrity check fails, the blob
  will be deleted;
+ currently supported component blobs are `application/octet-stream` (for binaries
  that are not decompressed) and `application/x-xz` (for compressed components);
