# Scripts

A place for collecting miscellaneous common development flows, and keeping these automations up-to-date.

# Setup

Be sure that your 1password cli integration is set up and that you have the nix developer environment (used by the rest of this repo too) working.

## Flows

1. Connecting to Stage Mongo: `teleport-mongo-stage.sh`
1. Downloading Ready-to-Sign images: See [orb-hil][hil]
1. Flashing dev Orbs: See [orb-hil][hil]
1. Inserting signup + attestation keys in the staging DB, for provisioning dev Orbs. Don't forget to set `active: true`.: `TODO`
1. Downloading the Debug Report + PCP from staging AWS: `TODO`
1. Running Orb Core with livestream / no encryption: `TODO`
1. Replace initrd on a running Orb: `TODO`
1. Generate a ROC licence for a dev Orb: `TODO`

[hil]: ../hil
