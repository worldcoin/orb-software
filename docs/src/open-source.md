# How we do open source

Worldcoin is committed to building a fully open source, decentralized
ecosystem. In service of this goal, the entirety of the orb-software repo is
open source under an MIT/Apache 2.0 dual license. You can read more about the
project's open sourcing efforts [here][blog].

All source code for the Worldcoin Project lives under the [worldcoin github
organizaton][github org].

## Overview of private repos

To the maximum extent possible, we put all public code in the orb-software and
orb-firmware repos. But we also maintain private repos, which contain code that
is responsible for fraud detection or uses third party SDKs that we do not have
a license to open source.

The most notable private repos are:

- [priv-orb-firmware][priv-orb-firmware]. This repo contains the fraud
  sensitive parts of the firmware. It consumes the public
  [orb-firmware][orb-firmware] repo as a dpenedency.
- [orb-internal][orb-internal]. This repo contains the fraud sensitive parts of
  the user-space software. It consumes the public [orb-software][orb-software]
  repo as a dependency.
- [priv-orb-core][priv-orb-core]. This repo is the mainline branch of
  `orb-core`. Unlike the other repos, this is a fork of its public counterpart,
  [orb-core][orb-core]. The public repo is therefore inherently less up to date
  and doesn't retain git history, as its code has all fraud-related codepaths
  manually deleted. One of the long-term goals is for these two repos to become
  un-forked, and most code consolidated into `orb-software` so that we can
  transition to developing orb-core directly in the open.
- [trustzone][trustzone]. This repo contains code related to the secure
  operating system OP-TEE that runs alongside linux inside ARM TrustZone. We
  plan to open source the OP-TEE CAs and TAs at some point in the future.-
- [orb-os][orb-os]. This repo is where we build the operating system image that
  runs on the orb. It consumes artifacts from all the other repos to assemble
  one final image.
- [orb-update-agent][orb-update-agent]. Contains code for OTAing orbs. We plan
  to open source this by merging it into the `orb-software` repo.
- [orb-update-verifier][orb-update-verifier]. Contains code used during the OTA
  process to check that the update booted successfully. We plan to open source
  this by merging it into the `orb-software` repo.


[blog]: https://worldcoin.org/blog/engineering/worldcoin-foundation-open-sources-core-components-orb-software
[github org]: https://github.com/worldcoin
[open-sourcing]: https://worldcoin.org/blog/engineering/worldcoin-foundation-open-sources-core-components-orb-software
[orb-core]: https://github.com/worldcoin/orb-core
[orb-firmware]: https://github.com/worldcoin/orb-firmware
[orb-internal]: https://github.com/worldcoin/orb-internal
[orb-os]: https://github.com/worldcoin/orb-os
[orb-software]: https://github.com/worldcoin/orb-software
[orb-update-agent]: https://github.com/worldcoin/orb-update-agent
[orb-update-verifier]: https://github.com/worldcoin/orb-update-verifier
[priv-orb-core]: https://github.com/worldcoin/priv-orb-core
[priv-orb-firmware]: https://github.com/worldcoin/priv-orb-firmware
[trustzone]: https://github.com/worldcoin/TrustZone
