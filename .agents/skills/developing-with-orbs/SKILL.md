---
name: developing-with-orbs
description: Use when working against a physical Orb from this repo, especially for SSH or scp access, deploying crates with cargo x deploy, using the orb-hil CLI, or running orb-mcu-util workflows
---

# Developing With Orbs

## Overview

Use this skill when the task needs a real Orb instead of local-only work. Favor
direct SSH and `scp` for inspection and staging files, `cargo x d` for deploying
workspace crates, `orb-hil` for hardware-in-loop workflows, and
`orb-mcu-util` for MCU inspection and firmware operations on the device.

## Quick Access

An Orb with ID `1234` is reachable over Avahi as:

```bash
ssh worldcoin@orb-1234.local
```

`scp` uses the same host naming:

```bash
scp ./local-file worldcoin@orb-1234.local:/tmp/local-file
scp worldcoin@orb-1234.local:/tmp/remote-file ./remote-file
```

If you already know the Orb IP, replace `orb-1234.local` with that IP.

## Deploying Crates From This Repo

Use `cargo x d` as the short form of `cargo x deploy` from the workspace
`xtask` crate:

```bash
export ORB_IP=orb-1234.local
export WORLDCOIN_PW='...'
cargo x d orb-mcu-util
```

Example:

```bash
cargo x d orb-mcu-util
```

Operational notes:

- `ORB_IP` can be a raw IP address or an Avahi host such as `orb-1234.local`.
- `WORLDCOIN_PW` should be set before deploys. The current xtask implementation
  will prompt interactively if either variable is unset, but for repeatable
  runs, set both env vars explicitly.
- `cargo x d <crate-name>` builds the crate for
  `aarch64-unknown-linux-gnu`, creates a `.deb`, copies it to the Orb, and
  reinstalls it there.
- If the crate declares systemd units in its package metadata, `cargo x d`
  automatically restarts the associated service on the Orb after install.

Prefer `cargo x d` when you are deploying a workspace crate. Prefer raw `scp`
when you are just staging an artifact or test file.

## Copying Files With scp

Push a file onto the Orb:

```bash
scp ./artifact.bin worldcoin@orb-1234.local:/tmp/artifact.bin
```

Copy a file back from the Orb:

```bash
scp worldcoin@orb-1234.local:/tmp/log.txt ./log.txt
```

Common pattern for MCU work:

```bash
scp ./target/aarch64-unknown-linux-gnu/release/main-mcu.bin \
  worldcoin@orb-1234.local:/tmp/main-mcu.bin
ssh worldcoin@orb-1234.local \
  'orb-mcu-util image update main --path /tmp/main-mcu.bin'
```

## Using the HIL CLI

The repo ships a hardware-in-loop CLI as `orb-hil`.

Run it from source:

```bash
AWS_PROFILE=hil aws sso login
AWS_PROFILE=hil cargo run -p orb-hil -- --help
```

Peripheral requirements matter:

- `orb-hil flash` needs an x86 Linux machine
- `orb-hil reboot` needs a serial adapter
- `orb-hil login` needs a serial adapter
- `orb-hil cmd` can work with a serial adapter or network access such as
  SSH/Teleport

Do not recommend `orb-hil reboot` or `orb-hil login` as SSH-only replacements.

Important command families from `hil/src/main.rs`:

- `button-ctrl`
- `cmd`
- `fetch-persistent`
- `flash`
- `login`
- `mcu`
- `nfsboot`
- `ota`
- `ping`
- `reboot`
- `set-recovery-pin`

Examples:

```bash
AWS_PROFILE=hil cargo run -p orb-hil -- flash --help
AWS_PROFILE=hil cargo run -p orb-hil -- reboot --help
AWS_PROFILE=hil cargo run -p orb-hil -- cmd --help
AWS_PROFILE=hil cargo run -p orb-hil -- mcu --help
```

Use `orb-hil` when you need hardware-in-loop flows such as flashing, reboot
orchestration, login automation, or command execution through supported HIL
transport paths.

## Using orb-mcu-util

`orb-mcu-util` is the direct MCU utility in this repo. It supports both normal
and CAN-FD operation; add `--can-fd` when the task specifically needs that bus.

Core inspection and reboot commands:

```bash
ssh worldcoin@orb-1234.local 'orb-mcu-util info'
ssh worldcoin@orb-1234.local 'orb-mcu-util info --diag'
ssh worldcoin@orb-1234.local 'orb-mcu-util reboot main'
ssh worldcoin@orb-1234.local 'orb-mcu-util reboot security'
ssh worldcoin@orb-1234.local 'orb-mcu-util reboot orb'
ssh worldcoin@orb-1234.local 'orb-mcu-util reboot --delay 30 orb'
ssh worldcoin@orb-1234.local 'orb-mcu-util reboot-behavior button'
ssh worldcoin@orb-1234.local 'orb-mcu-util reboot-behavior always-on'
```

Firmware image commands:

```bash
ssh worldcoin@orb-1234.local \
  'orb-mcu-util image update main --path /tmp/main-mcu.bin'
ssh worldcoin@orb-1234.local \
  'orb-mcu-util image update security --path /tmp/security-mcu.bin'
ssh worldcoin@orb-1234.local \
  'orb-mcu-util image update main --path /tmp/main-mcu.bin --force'
ssh worldcoin@orb-1234.local 'orb-mcu-util image switch main'
ssh worldcoin@orb-1234.local 'orb-mcu-util image switch security'
ssh worldcoin@orb-1234.local 'orb-mcu-util image force-switch main'
```

Dump and stress commands:

```bash
ssh worldcoin@orb-1234.local 'orb-mcu-util dump main --duration 30'
ssh worldcoin@orb-1234.local 'orb-mcu-util dump security --duration 30 --logs-only'
ssh worldcoin@orb-1234.local 'orb-mcu-util stress main --duration 30'
ssh worldcoin@orb-1234.local 'orb-mcu-util stress security --duration 30'
```

Hardware, power, and peripheral commands:

```bash
ssh worldcoin@orb-1234.local 'orb-mcu-util hardware-revision'
ssh worldcoin@orb-1234.local 'orb-mcu-util power-cycle secure-element'
ssh worldcoin@orb-1234.local 'orb-mcu-util power-cycle heat-camera'
ssh worldcoin@orb-1234.local 'orb-mcu-util power-cycle wifi'
ssh worldcoin@orb-1234.local 'orb-mcu-util ui front red'
ssh worldcoin@orb-1234.local 'orb-mcu-util ui front white'
ssh worldcoin@orb-1234.local 'orb-mcu-util optics gimbal-home autohome'
ssh worldcoin@orb-1234.local 'orb-mcu-util optics gimbal-position --phi 45000 --theta 90000'
ssh worldcoin@orb-1234.local 'orb-mcu-util optics gimbal-move --phi 100 --theta -100'
ssh worldcoin@orb-1234.local 'orb-mcu-util optics trigger-camera eye 30'
ssh worldcoin@orb-1234.local 'orb-mcu-util optics trigger-camera face 30'
ssh worldcoin@orb-1234.local 'orb-mcu-util optics polarizer home'
ssh worldcoin@orb-1234.local 'orb-mcu-util optics polarizer passthrough'
ssh worldcoin@orb-1234.local 'orb-mcu-util optics polarizer vertical'
ssh worldcoin@orb-1234.local 'orb-mcu-util optics polarizer horizontal'
ssh worldcoin@orb-1234.local 'orb-mcu-util optics polarizer angle 900'
ssh worldcoin@orb-1234.local \
  'orb-mcu-util optics polarizer stress 0 100 --random'
ssh worldcoin@orb-1234.local \
  'orb-mcu-util optics polarizer settings --acceleration 100 --max-speed 100'
```

Use these patterns:

- `info` or `info --diag` to inspect current MCU state
- `reboot` and `reboot-behavior` to control board or Orb restart behavior
- `image update` after staging a firmware binary with `scp`
- `image switch` or `image force-switch` to change active image slots
- `dump` to collect MCU messages, optionally logs only
- `stress` to exercise MCU communication
- `power-cycle` for secure element, heat camera, or Wi-Fi power resets
- `ui` and `optics` when the task involves front LEDs, gimbal, cameras, or the
  polarizer
- `optics polarizer stress <speed> <repeat>` uses positional arguments; in the
  example above, `0 100` means speed `0` and `100` repetitions

For anything beyond the examples above, run:

```bash
ssh worldcoin@orb-1234.local 'orb-mcu-util --help'
ssh worldcoin@orb-1234.local 'orb-mcu-util image --help'
ssh worldcoin@orb-1234.local 'orb-mcu-util optics --help'
ssh worldcoin@orb-1234.local 'orb-mcu-util ui --help'
```

## Source Of Truth

When the command surface matters, read these files directly:

- `xtask/src/main.rs`
- `xtask/src/cmd/deploy.rs`
- `docs/src/hil/cli.md`
- `hil/src/main.rs`
- `mcu-util/src/main.rs`
