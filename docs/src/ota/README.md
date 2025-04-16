# Over-The-Air Updates (OTAs)

## Overview of components

The OTA system for the orb is comprised of several parts. All components are FOSS
unless stated otherwise.
Backend infrastructure is owned by Worldcoin Foundation and operated by Tools For
Humanity, unless otherwise noted.

It is a long term goal to make these components FOSS, and decentralize them as much
as possible.

**OTA Consumption (On the orb)**:
* [`orb-update-agent`][orb-update-agent] binary.
* [`orb-update-verifier`][orb-update-verifier] binary.

**OTA Production (Backend)**
* [`orb-os`][orb-os] repo: Not yet FOSS 😢. Contains build scripts and CI to produce the
  custom debian based linux distro we ship on orbs.
* [`orb-bidiff-cli`][orb-bidiff-cli]: Produces binary diffs of OTAs for download size
  reduction.
* [`orb-updates-lambda`][orb-updates-lambda]: Not FOSS 😢. Legacy golang lambda that
  runs when `orb-os` builds an OTA, to post-process it.
* [`orb-manager`][orb-manager]: Not FOSS 😢. Legacy golang endpoints for OTA updates
  being migrated to `orb-fleet-backend`.

**OTA Consumption (Backend)**
* `orb-manager`: see above.
* `orb-fleet-backend`: Not FOSS 😢. New rust/axum service that will manage orbs.
* AWS S3 buckets: Used to persist orb-os builds, OTAs, and bidiffs.
* MongoDB: Tracks various information about OTAs and which orbs they are assigned to.

[orb-bidiff-cli]: https://github.com/worldcoin/orb-software/tree/main/bidiff-cli
[orb-manager]: https://github.com/worldcoin/orb-manager
[orb-os]: https://github.com/worldcoin/orb-os
[orb-update-agent]: https://github.com/worldcoin/orb-software/tree/main/update-agent
[orb-update-verifier]: https://github.com/worldcoin/orb-software/tree/main/update-verifier
[orb-updates-lambda]: https://github.com/worldcoin/orb-manager/tree/0d46e5e4148ec514fd0c67624f43d58711094092/updates-lambda
