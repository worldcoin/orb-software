# Orb Service Map

Read the section that matches the symptom. Query the primary service first, then add related services only when the timeline or dependency chain calls for them. Confirm every deployed Datadog `service` tag from an Orb-wide query because unit names and tags can differ across Orb generations.

## General health

- `check-my-orb`: on-demand composite diagnostic. Its output can cover OS release, hardware platform, active update slot, mounts, model artifacts, persistent storage, service state, SE050 operations, and MCU firmware versions. Remote jobs can return its output through `worldcoin-jobs-agent`.
- `worldcoin-update-verifier`: checks system health after an update and manages slot and rootfs approval.
- `datadog-agent`: inspect when multiple service logs disappear or the Orb has no recent telemetry.

## MCU health

- `worldcoin-orb-telemetry`: primary MCU telemetry service supplied by the operator runbook.
- `worldcoin-mcu-telemetry`: older observed name; use only when the Orb-wide query returns it.
- `worldcoin-configure-can@can0`: CAN setup used by attestation and MCU-aware services.

## Connectivity

- `worldcoin-connd`: Orb connectivity daemon for Wi-Fi, cellular, and Bluetooth. Its unit requires `NetworkManager` and `zenohd`.
- `NetworkManager`: connection profiles, interface state, routes, and failover.
- `ModemManager`: cellular modem state and recovery.
- `wpa_supplicant` or platform-specific variants: Wi-Fi authentication and control.
- `systemd-networkd`: older or platform-specific network management; query only when observed.
- `zenohd`: messaging dependency whose failure can affect connd and several Orb services.

## Attestation

- `worldcoin-attest`: retrieves and refreshes the short-lived backend authorization token through the SE050 and exposes it on D-Bus.
- `worldcoin-se050-reprovision`: SE050 reprovisioning; relevant after provisioning or secure-element failures.
- `worldcoin-dbus`: session bus used to distribute the attestation token.
- `zenohd` and `worldcoin-configure-can@can0`: soft dependencies and hardware communication paths.

## Updates

- `worldcoin-supervisor`: coordinates privileged device state and shutdown behavior.
- `worldcoin-update-agent`: fetches and installs update components; coordinates with supervisor and update storage mounts.
- `worldcoin-update-verifier`: waits for system stability, then approves or rejects the active slot.
- `mnt-scratch.mount`, `mnt-updates.mount`, `worldcoin-ssd-setup-scratch`, and `worldcoin-ssd-setup-models`: inspect when downloads, artifacts, models, or slot verification fail.

## Remote jobs

- `worldcoin-jobs-agent`: receives, executes, cancels, and reports prescribed remote jobs.
- `worldcoin-attest`: supplies backend authorization.
- `zenohd`: carries the job messaging path.
- `datadog-agent`: relevant when job execution appears silent rather than failed.

## Backend reporting

- `worldcoin-backend-status`: collects Orb status over D-Bus and sends it to the fleet backend.
- `worldcoin-attest`, `worldcoin-dbus`, and `zenohd`: hard dependencies in its unit.

## Signups

- `worldcoin-core`: owns signup logic and sensor orchestration.
- `worldcoin-ui`: presents operator and user feedback.
- `worldcoin-supervisor`: controls device state that can permit or block signups.
- `worldcoin-attest` and `worldcoin-connd`: backend authorization and connectivity.
- `worldcoin-backend-status`: observes and reports signup lifecycle states. Distinguish not-ready, in-progress, completed-failure, hung, and successful-but-unreported signups.

## Observability and shared IPC

- `datadog-agent`: log and metric collection and forwarding.
- Orb monitoring-auth client/server: supplies Datadog monitoring credentials; discover the exact deployed service tags before filtering.
- `worldcoin-dbus`: shared local IPC bus.
- `zenohd`: shared messaging transport. Correlate it when several dependent services fail together.

## Repository sources

- Unit files: `*/debian/*.service`
- Composite health example: `orb-jobs-agent/tests/docker/check-my-orb_output.txt`
- Component descriptions: top-level `README.md`, component `README.md`, and component `Cargo.toml`
- Connectivity implementation: `orb-connd/`
- Telemetry transport: `telemetry/`, `orb-dogd/`, and `orb-monitoring-auth/`
