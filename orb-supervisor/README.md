# Orb Supervisor

Orb supervisor is a central IPC server that coordinates device state and external UX across independent agents (binaries/processes).

## Table of Contents

- Minimal viable product (MVP)
- Why (this is necessary)
    - Managing device health
    - Consistent UX
    - Seperation of concerns
- Relevant components

## MVP

### Initial release

- supervisor running [tonic gRPC](https://github.com/hyperium/tonic) over UDS (Unix Domain Sockets)
- supervisor can broadcast shutdown message
    - component apps (orb-core, update-agent) listen for broadcast and shutdown
- supervisor can update SMD **through sub-process**
- supervisor can display front LED patterns
- IPC (InterProcess-Communication) client library supporting defaults for process shutdown handlers
    - Setup the bidirectional communication + the listener for broadcast messages

### Immediate follow-up release
- supervisor can play sounds
- supervisor can engage in bi-directional communication for signup permission with orb-core; orb-core must not run a signup if...
    - an update is scheduled;
    - the device is shutting down;
    - the SSD is full (coordinate with @AI on signup extensions);
- fan-controller PoC
    - spin fans up/down depending on temperature/temperature-analogs
    - watch iops/sec on NVMe as an indicator of SSD temperature (can be replaced by reading out SMART data after kernel 5.10 is deployed)
- supervisor can update SMD **through nvidia-smd crate**
    - Implement an Nvidia SMD parser as a crate (other people may want this)

## Why this is necessary

There are two reasons that make the orb supervisor necessary:

1. Managing device health (heat, updates)
1. Consistent UX (updates w/ voice, LEDs)
1. Separation of concerns

### Managing device health

Device health must be ensured at all times, whether the device is updating or in the middle of a signup. Furthermore, you want this to be maximally isolated to avoid a scenario where, through a vulnerability in a monolithic application, an attacker acquires fan control and overheats the device. 

> **Scenario**: _A non-security critical update is running in the background and writing large blobs of data to the NVMe SSD_ while _orb-core is running and signups are being performed. An attacker uses a vulnerability in the QR code processing to deadlock a thread. They then proceed to garble the incoming network traffic causing the download to be repeatedly retried and data to be constantly written to the SSD while thermal management is stuck in the blocked runtime. This can feasibly fry an Orb._

### Consistent UX

By necessity, the update agent service must have heightened privileges. Under no circumstances can we extend these to the entire orb-core process. At the same time, the operator must receive feedback on the status of an update. For certain updates, orb-core will not run during the update. In this scenario there is currently no mechanism to give feedback to the operator.

Thus, an independent service that owns UX is a necessary condition for operator feedback. 

### Seperation of concerns

Breaking components down allows us to:

+ Reduce attack surfaces by restricting the responsibilities of privileged services;
+ Employ best patterns for the job (a fan monitoring service looks different from an update agent looks different from orb core);
+ Reduce engineering load (understanding a 500 LoC binary and finding bugs _is_ easier than in a 10k LoC monolith);
+ Running integration tests is significantly easier outside of complex runtimes.

It is best industry practice to write dedicated services *where possible*, where coupling is low and where solutions already exist. This applies especially on a full Linux host and will reduce engineering load.

## Relevant components

+ update agent
+ fan monitor & control
+ wifi management
+ UX controller, split into:
    + Sound
    + LED
+ library for basic and repeatable "component"
