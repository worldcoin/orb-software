# orb-connd Architecture

Orb Connectivity Daemon — manages WiFi, cellular, and network configuration on the Orb.

## Crate Layout

```
orb-connd/
├── src/                        Main binary + library
│   ├── main.rs                 CLI entry point (two subcommands)
│   ├── lib.rs                  Public API surface
│   ├── connectivity_daemon.rs  Program builder, capability detection, startup
│   ├── utils.rs                State<T>, retry_for(), IntoZResult
│   ├── resolved.rs             systemd-resolved DNS client (D-Bus)
│   ├── network_manager/        NetworkManager D-Bus wrapper
│   ├── modem_manager/          ModemManager abstraction (mmcli)
│   ├── wpa_ctrl/               wpa_cli wrapper
│   ├── service/                Business logic + D-Bus method handlers
│   ├── reporters/              Async telemetry/status reporting tasks
│   ├── statsd/                 DogStatsD abstraction
│   └── secure_storage/         Encrypted profile persistence (OP-TEE subprocess)
├── dbus/                       orb-connd-dbus: D-Bus interface trait + types
├── events/                     orb-connd-events: rkyv-serialized pub/sub types
├── tests/                      Docker-based integration tests
├── nm_cfg/                     NetworkManager drop-in config
└── debian/                     Systemd service unit
```

## Binary Entry Points

`main.rs` defines two subcommands via `clap`:

1. **`ConnectivityDaemon`** — the main daemon. Initializes telemetry, D-Bus
   connections, builds the service, spawns reporters, and awaits SIGTERM/SIGINT.
2. **`SecureStorageWorker`** — a privilege-isolated subprocess that handles
   encrypted profile persistence via OP-TEE. Runs as a dedicated system user
   (`orb-ss-connd-nmprofiles`) and communicates with the parent over
   stdin/stdout using length-delimited CBOR frames.

## Data Flow

```
External callers (UI, backend agents)
        │
        ▼
D-Bus interface (org.worldcoin.Connd1)
        │
        ▼
   ConndService
        │
        ├──► NetworkManager        (D-Bus)
        ├──► Secure Storage        (subprocess IPC, CBOR)
        ├──► ModemManager          (mmcli CLI)
        ├──► wpa_cli               (CLI)
        └──► systemd-resolved      (D-Bus)
        │
        ▼
   Reporters (async tasks)
        │
        ├──► Zenoh pub/sub         (net/changed, oes/active_connections)
        ├──► Backend Status        (D-Bus)
        └──► Datadog StatsD        (UDP)
```

## Key Modules

### `service/` — Business Logic

The core of the daemon. `ConndService` holds shared state and implements all
D-Bus methods defined in the `ConndT` trait.

| File            | Responsibility                                              |
|-----------------|-------------------------------------------------------------|
| `mod.rs`        | Service construction, startup sequence, profile sync        |
| `dbus.rs`       | D-Bus method implementations (add/remove/list profiles, scan, connect, netconfig) |
| `wifi.rs`       | MECARD WiFi QR code parsing and application                 |
| `netconfig.rs`  | NETCONFIG v1.0 QR parsing with P256 ECDSA signature verification |
| `wpa_conf.rs`   | Legacy `wpa_supplicant.conf` import on first boot           |
| `mecard.rs`     | Low-level MECARD field parser (handles escaping)            |

**Startup sequence** (`ConndService::init`):
1. Wait for NetworkManager readiness
2. Install default profiles (cellular APN, hotspot WiFi)
3. Import legacy `wpa_supplicant.conf` if present
4. Import persisted profiles from secure storage (Diamond only)
5. Prune oversized NetworkManager state files (>1 MB)

### `network_manager/` — NetworkManager D-Bus Wrapper

A comprehensive async wrapper (~1100 lines) around NetworkManager's D-Bus API
via the `rusty_network_manager` crate. Handles:

- Profile CRUD (WiFi and cellular)
- Connection activation / listing active connections
- WiFi scanning with signal strength, security flags
- State change subscriptions (streams)
- Smart switching, airplane mode, WiFi/WWAN enable/disable

Key types: `Connection` (enum: Wifi/Cellular/Ethernet), `WifiProfile`,
`CellularProfile`, `AccessPoint`, `WifiSec`, `ActiveConn`.

### `modem_manager/` — Cellular Modem Control

Trait-based abstraction over the `mmcli` CLI tool. The trait `ModemManager`
defines async methods for modem enumeration, signal queries, SIM info, location,
and band/mode configuration. `cli.rs` implements it by parsing `mmcli` JSON
output.

### `reporters/` — Telemetry Tasks

Each reporter is a long-lived async task spawned at startup:

| Reporter                            | What it publishes                                   | Where        |
|-------------------------------------|-----------------------------------------------------|--------------|
| `net_changed_reporter`              | Network state transitions (Connected/Disconnected)  | Zenoh        |
| `active_connections_report`         | DNS + HTTP connectivity checks, connection details  | OES metrics  |
| `backend_status_wifi_reporter`      | WiFi profiles, nearby networks                      | D-Bus        |
| `backend_status_cellular_reporter`  | Cellular signal, IMEI                               | D-Bus        |
| `modem_monitor`                     | Periodic modem info polling                         | Internal     |
| `dd_modem_reporter`                 | Signal strength, interface byte counters            | Datadog      |

### `secure_storage/` — Encrypted Profile Persistence

Used on Diamond Orbs to persist WiFi profiles in OP-TEE secure storage.

- **Parent side** (`mod.rs`): `SecureStorage` async client sends `Request`
  variants (Store, LoadAll, Delete) over a framed CBOR channel.
- **Subprocess side** (`subprocess.rs`): Spawned as `SecureStorageWorker`
  subcommand, drops privileges to a dedicated user, processes requests against
  the OP-TEE backend.
- **Test mode**: Swaps in `InMemoryBackend` for integration tests.

### `resolved.rs` — DNS Client

Thin async client for `systemd-resolved` over D-Bus. Provides hostname
resolution (with per-interface scoping) and DNS server status queries.

## D-Bus Interface

Defined in the `orb-connd-dbus` crate as the `ConndT` trait.

- **Service**: `org.worldcoin.Connd`
- **Interface**: `org.worldcoin.Connd1`
- **Path**: `/org/worldcoin/Connd1`

Methods include: `add_wifi_profile`, `remove_wifi_profile`, `list_wifi_profiles`,
`scan_wifi`, `connect_to_wifi`, `netconfig_set`, `netconfig_get`,
`apply_wifi_qr`, `apply_netconfig_qr`, `apply_magic_reset_qr`,
`connection_state`.

Shared types (`WifiProfile`, `AccessPoint`, `NetConfig`, `ConnectionState`) are
defined in `dbus/src/lib.rs` with `zbus::zvariant` derives.

## Event Types

The `orb-connd-events` crate defines rkyv-serialized types for Zenoh pub/sub:

- `Connection`: Disconnected, Connecting, Disconnecting, ConnectedLocal,
  ConnectedSite, ConnectedGlobal
- `ConnectionKind`: Wifi { ssid }, Cellular { apn }, Ethernet

## Platform Differences

| Aspect              | Pearl (WiFi-only)                | Diamond (WiFi + Cellular)                  |
|---------------------|----------------------------------|--------------------------------------------|
| Capabilities        | `WifiOnly`                       | `CellularAndWifi`                          |
| Profile storage     | NetworkManager (filesystem)      | Secure storage subprocess (OP-TEE)         |
| wpa_cli path        | `/sbin/wpa_cli`                  | `/usr/sbin/wpa_cli`                        |
| Cellular reporters  | Not spawned                      | Modem monitor, signal/location reporting   |

Platform is detected at startup by probing sysfs for modem presence.

## QR Code Formats

**WiFi QR (MECARD)**:
```
WIFI:T:<auth>;S:<ssid>;P:<password>;H:<hidden>;;
```

**NetConfig QR (v1.0)** — extends MECARD with network settings + signature:
```
NETCONFIG:v1.0;T:<auth>;S:<ssid>;P:<password>;H:<hidden>;WIFI_ENABLED:<bool>;AIRPLANE:<bool>;SMART_SWITCHING:<bool>;TS:<epoch>;SIG:<p256_ecdsa_sig>;;
```

NetConfig QR codes are verified against environment-specific P256 ECDSA public
keys (prod/stage/dev) embedded in the binary.

## Testing

Integration tests use Docker containers running NetworkManager, dbus-daemon, and
zenohd. The test fixture (`tests/fixture.rs`) provides:

- Isolated D-Bus bus per test
- Mock implementations of `ModemManager`, `StatsdClient`, `WpaCtrl`
- In-memory secure storage backend
- Simulated sysfs and NetworkManager state directories

Test coverage spans profile CRUD, QR code parsing, netconfig operations,
profile persistence across restarts, and legacy config import.
