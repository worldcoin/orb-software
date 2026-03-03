# OES (Orb Event Stream)

The Orb Event Stream is a real-time stream of messages sent from the orb to the backend.
It can be used to both publish from any process to the backend by using a specific topic schema, but also by implementing rerouters in `orb-backend-status`.

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

### Rerouting existing non-OES topics to the OES
This can be done in the main `zenoh::Receiver` in `orb-backend-status`. Simply call the extension method `oes_reroute` and define a throttle (any message after the first within throttle period is dropped).
```rust
.oes_reroute(
    "core/config",
    Duration::from_millis(100),
    Duration::from_secs(1),
)
```

### How this reaches the backend

Events are forwarded to the same backend status endpoint used by
`orb-backend-status` (see `src/backend/status.rs`). The request payload uses
the same `OrbStatusApiV2` schema, but only the `oes` field is populated:

```rust
struct OrbStatusApiV2 {
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

## Events

### `connd/active_connections`

Published by `orb-connd` whenever the primary network connection changes.
Reports the state of every active NetworkManager connection, including DNS
resolution and an HTTP connectivity check per interface.

See [src/connd.rs](src/connd.rs) for the full `ActiveConnections` struct.

#### Example

```json
{
  "connectivity_uri": "http://connectivity-check.worldcoin.org",
  "connections": [
    {
      "name": "TFHOrbs",
      "iface": "wlan0",
      "primary": false,
      "has_internet": false
    },
    {
      "name": "Wired connection 1",
      "iface": "eth0",
      "primary": true,
      "has_internet": true
    }
  ]
}
```
