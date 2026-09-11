# OES (Orb Event Stream)

The Orb Event Stream is a stream of real-time or cached messages sent from the orb to the backend.
It can be used to both publish from any process to the backend by using a specific topic schema, but also by implementing rerouters in `orb-backend-status`.

> Every event published on the OES **must** have its struct declared here, and **must** be converted into that struct before publishing.

## How to Publish on the OES

### Publishing Specifically to the OES
Any process on the Orb can publish to OES by using zenorb and publishing to
`oes/<event_name>` with encoding set to `application/json` or
`application/text`. These events are automatically forwarded to the backend.

Below is an example using `zenorb::Sender`.

```rust
zsender
    .publisher("oes/active_connections")?
    .put(&bytes)
    .await
```

### OES Delivery Modes
`oes::Headers` controls how `orb-backend-status` handles an event after it is
received. If no headers are attached, the mode defaults to `oes::Mode::Normal`.

- `oes::Mode::Normal`: forwards the event to the backend immediately. `backend-status` does not cache it.
- `oes::Mode::Sticky`: forwards the event to the backend immediately and also caches the latest value for that event name.
- `oes::Mode::CacheOnly`: updates the cache only. Does not forward the event immediately.

Cached events are included in the periodic backend-status snapshot (every 30s), so
`Sticky` and `CacheOnly` are both appropriate for “latest known value” style
state.

To set the `oes::Mode` for an event, simply set an `oes::Headers` instance as an attachment when publishing a zenoh message.

```rust
zsender
    .publisher("oes/my_event")?
    .put(&bytes)
    .attachment(oes::Headers::default().mode(oes::Mode::Sticky))
    .await
```

Buffered publishers should capture the event's Unix-millisecond occurrence time
before enqueueing and attach it with
`oes::Headers::default().created_at(occurrence_ms)`. Keep that timestamp unchanged
across retries. `orb-backend-status` uses it as the envelope's `created_at`; absent
or out-of-range timestamps retain the legacy receipt-time behavior. Deploy this
receiver support before enabling buffered producers so replayed startup events
do not appear newer than subsequent lifecycle evidence.


### Rerouting existing non-OES topics to the OES
This can be done *only* in the main `zenoh::Receiver` in `orb-backend-status`. Simply call the extension method `oes_reroute` 
```rust
.oes_reroute(
    "core/config",
    Duration::from_millis(100),
    oes::Mode::CacheOnly
)
```

### How this reaches the backend

Events are forwarded to the same backend status endpoint used by
`orb-backend-status` (see `src/backend/status.rs`). The request payload uses
the same `OrbStatusApiV2` schema, but only the `oes` field is populated:

```rust
struct OrbStatusApiV2 {
    oes_cached: bool, // true only whenever cached events are sent (every 30s)
    oes: Option<Vec<Event>>,
    // ... all other fields omitted (all Optional) ...
}

struct Event {
    name: String,
    created_at: i64, // milliseconds since unix epoch
    payload: Option<serde_json::Value>,
}
```

The `oes` field is a list of events. Each event has a `name` (e.g.
`"connd/active_connections"`), a `created_at` field (milliseconds since unix
epoch), and an optional JSON `payload`.

---

## `orb-connd` Events

### `connd/active_connections`
- **Frequency**: on change
- **Mode**: `oes::Sticky`

```
Event {
  name: "connd/active_connections"
  payload: ActiveConnections
}
```

Published by `orb-connd` whenever the primary network connection changes.
Reports the state of every active NetworkManager connection.
See [src/connd.rs](src/connd.rs) for the full `ActiveConnections` struct.

#### Payload Example

```json
{
  "connectivity_uri": "http://connectivity-check.worldcoin.org",
  "connections": [
    {
      "name": "TFHOrbs",
      "iface": "WiFi",
      "primary": false,
      "has_internet": false
    },
    {
      "name": "Wired connection 1",
      "iface": "Cellular",
      "primary": true,
      "has_internet": true
    }
  ]
}
```

### `connd/netstats`
- **Frequency**: 30s
- **Mode**: `oes::CacheOnly`

```
Event {
  name: "connd/netstats"
  payload: Vec<NetStats>
}
```

Published by `orb-connd` to the OES cache every 30s. Gathers sent and received bytes for `eth0`, `wwan0` and `wlan0`.
See [src/connd.rs](src/connd.rs) for the full `NetStats` struct.

#### Payload Example

```json
  [
    {
      "iface": "eth0",
      "tx_bytes": 123456,
      "rx_bytes": 654321
    },
    {
      "iface": "wwan0",
      "tx_bytes": 45678,
      "rx_bytes": 98765
    },
    {
      "iface": "wlan0",
      "tx_bytes": 77777,
      "rx_bytes": 88888
    }
  ]
```

### `connd/cellular_status`
- **Frequency**: 30s
- **Mode**: `oes::Normal`

```
Event {
  name: "connd/cellular_status"
  payload: CellularStatus
}
```

Published by `orb-connd` to the OES every 30s. Gathers Information about the orb's current cellular status.
If no events are seen for longer than a minute, the orb is likely to be having issues with cellular connectivity or the modem itself.

See [src/connd.rs](src/connd.rs) for the full `CellularStatus` struct.

#### Payload Example

```json
  {
    "imei": "123456789012345",
    "fw_revision":"25.30.608  1  [Nov 14 2023 07:00:00]",
    "iccid": "8945738730000000000",
    "rat": "lte",
    "operator": "vodafone P",
    "rsrp": -92.5,
    "rsrq": -11.0,
    "rssi": -67.0,
    "snr": 14.2
  }
```

## `orb-core` Events

### `core/service_started`
- **Frequency**: on change
- **Mode**: `oes::Normal`

```
Event {
  name: "core/service_started"
  payload: ServiceStartedEvent
}
```

Published by `orb-core` when the service starts. Empty payload.
See [src/core.rs](src/core.rs).

### `core/bootstrap`
- **Frequency**: bootstrap failure state changes and successful bootstrap
- **Mode**: `oes::Normal`

Reports initialization before the operator/user QR flow. Failure identifies a
bounded stage (`network`, `token`, `configuration`, or `warmup`); it carries no
raw error, URL, token, or credential. A successful bootstrap clears a preceding
bootstrap error but does not assert readiness for a signup. Existing QR phase
events establish whether core is waiting for an operator or a user.

```json
{"state": "failed", "stage": "configuration"}
```

```json
{"state": "succeeded"}
```

### `core/operator_qr_restored`
- **Frequency**: a validated persisted operator credential becomes active
- **Mode**: `oes::Normal`

Empty payload (`{}`). Emitted on successful startup restoration and restoration
after a test operator credential. Reading the file, rejected validation, and
ordinary in-memory revalidation do not produce this event. It records credential
provenance, not a readiness transition, and contains no operator identifier or QR
contents.

### `core/qr_scan`
- **Frequency**: on change
- **Mode**: `oes::Normal`

```
Event {
  name: "core/qr_scan"
  payload: QrScanEvt
}
```

Published by `orb-core` during QR code scanning. Records the scanning phase
and outcome. See [src/core.rs](src/core.rs) for the full `QrScanEvt` struct.

Accepted operator QR events include optional `origin`: `camera` for a physical
scan (including refresh while waiting for a user), or `cli` for injected input.
Absent or unrecognized origin means unknown; a legacy success must not be treated
as proof of a physical scan. Stored credentials use `core/operator_qr_restored`.

#### Payload Example

```json
{
  "phase": "operator",
  "state": {
    "success": {
      "kind": "operator"
    }
  },
  "origin": "camera"
}
```

Fleet's API parser and typed models must be updated together with new event
names or fields. The generic backend-status transport forwards JSON unchanged,
but unsupported events can be discarded by the receiving application. Deploy
receiver support before enabling new producer events; keep existing log fallback
coverage until the OES rollout is verified.

### `core/config`
- **Frequency**: on change
- **Mode**: `oes::Normal`

```
Event {
  name: "core/config"
  payload: PublishableConfig
}
```

Rerouted from the `core/config` zenoh topic. Subset of orb-core config
exposed to backend-status and other services.
See [src/core.rs](src/core.rs) for the full `PublishableConfig` struct.

#### Payload Example

```json
{
  "thermal_camera_required": true
}
```

## System Events

### `system/boot_id`
- **Frequency**: on backend-status startup, then included in cached OES snapshots every 30s
- **Mode**: `oes::Mode::CacheOnly`

```
Event {
  name: "system/boot_id"
  payload: BootIdEvent
}
```

Cached by `orb-backend-status` from `/proc/sys/kernel/random/boot_id`.

#### Payload Example

```json
{
  "boot_id": "16e16562-856b-4a20-9b46-4574a9be1d19"
}
```
