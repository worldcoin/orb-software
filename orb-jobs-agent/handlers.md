# handlers documentation

this document is designed to be a documentation of what handles are available, what arguments do they take and what responses they can return.

## calling jobs-agent handlers
`jobs-agent` handlers can be called by fleet commander by their names, followed by arugments.

there are two ways of sending arumgents:
- positional args, like so: `<command_name> <arg1> <arg2> <arg3>`
  these are parsed with simple logic, and do not respect `"` or `'` or any other character as a way to work around whitespace splitting.
  example:
  ```
  read_file /usr/persistent/versions.json
  ```
- json args, like so: `<command_name> <serialized_json>`
  format must be command name, followed by whitespace, and then a serialized json data structure.
  the json arg will be deserialized and validated with serde
  example:
  ```
  wifi_add {"ssid":"tfh_orbs","pwd":"12345678","sec":"Wpa2Psk"}
  ```

## wifi_add

Adds a WiFi network profile to the system. Optionally connects to the network immediately if `join_now` is true.

**Command format:** `wifi_add <WifiAdd json>`

**Arguments:**
- `ssid`: String - The SSID (network name) of the WiFi network
- `sec`: String - Security type, one of the two types: "Wpa2Psk" or "Wpa3Sae" (feel free to use "Wpa3Sae" for WPA2/WPA3 transitional networks)
- `pwd`: String - Password for the network (must be at least 8 characters)
- `hidden`: Boolean (optional, default: false) - Whether the network is hidden
- `join_now`: Boolean (optional, default: false) - Whether to attempt to connect to the network immediately after adding the profile

**Note**: `hotspot` and `cellular` are protected ssid names and cannot be used when adding a new network.

**Example:**
```
wifi_add {"ssid":"HomeWIFI","sec":"Wpa3Sae","pwd":"12345678","hidden":false,"join_now":true}
```


**Response:** JSON object indicating connection status, and containing the AP if connected successfully (otherwise will contain null)
```json
{
  "connection_success": true,
  "network": {
    "ssid": "HomeWIFI",
    "bssid": "aa:bb:cc:dd:ee:ff",
    "is_saved": false,
    "is_active": false,
    "freq_mhz": 2412,
    "max_bitrate_kbps": 54000,
    "strength_pct": 85,
    "last_seen": "2023-01-01T12:00:00Z",
    "mode": "Ap",
    "capabilities": {...},
    "sec": "Wpa2Psk"
  }
}  // or false if connection failed, or null if join_now was false
```

## wifi_remove

Removes a saved WiFi network profile from the system.

**Command format:** `wifi_remove <ssid>`

**Arguments:**
- `ssid`: String - The SSID of the network to remove

**Example:**
```
wifi_remove TFHOrbs
```

**Response:** Success status with no additional output

## wifi_connect

Connects to a previously added WiFi network.

**Command format:** `wifi_connect <ssid>`

**Arguments:**
- `ssid`: String - The SSID of the network to connect to

**Example:**
```
wifi_connect TFHOrbs
```

**Response:** Success status with information about the access point if connected successfully.
```
  {
    "ssid": "HomeWIFI",
    "bssid": "aa:bb:cc:dd:ee:ff",
    "is_saved": false,
    "is_active": false,
    "freq_mhz": 2412,
    "max_bitrate_kbps": 54000,
    "strength_pct": 85,
    "last_seen": "2023-01-01T12:00:00Z",
    "mode": "Ap",
    "capabilities": {...},
    "sec": "Wpa2Psk"
  }
```

## wifi_list

Lists all saved WiFi network profiles.

**Command format:** `wifi_list`

**Arguments:** None

**Note**: `hotspot` will always be present as it is a default profile we keep saved in case things go wrong.

**Example:**
```
wifi_list
```

**Response:** JSON array of WiFiProfile objects
```json
[
  {
    "ssid": "HomeWIFI",
    "sec": "Wpa2Psk",
    "psk": "unencrypted_password",
    "is_active": true
  }
]
```

all possible sec values [here](https://github.com/worldcoin/orb-software/blob/49a5768b35d0bb5b1793f0db86c0dcd71bdde67c/orb-connd/src/network_manager/mod.rs#L652)

## wifi_scan

Scans for available WiFi access points in range.

**Command format:** `wifi_scan`

**Arguments:** None

**Example:**
```
wifi_scan
```

**Response:** JSON array of AccessPoint objects containing details about discovered networks
```json
[
  {
    "ssid": "HomeWIFI",
    "bssid": "aa:bb:cc:dd:ee:ff",
    "is_saved": false,
    "is_active": false,
    "freq_mhz": 2412,
    "max_bitrate_kbps": 54000,
    "strength_pct": 85,
    "last_seen": "2023-01-01T12:00:00Z",
    "mode": "Ap",
    "capabilities": {...},
    "sec": "Wpa2Psk"
  }
]
```

all possible sec values [here](https://github.com/worldcoin/orb-software/blob/49a5768b35d0bb5b1793f0db86c0dcd71bdde67c/orb-connd/src/network_manager/mod.rs#L652)

## netconfig_set

Sets the network configuration settings.

**Command format:** `netconfig_set <NetConfig json>`

**Arguments:**
- `wifi`: Boolean - Enable/disable WiFi
- `smart_switching`: Boolean - Enable/disable smart network switching
- `airplane_mode`: Boolean - Enable/disable airplane mode

**Example:**
```
netconfig_set {"wifi":true,"smart_switching":false,"airplane_mode":false}
```

**Response:** JSON object reflecting the applied configuration
```json
{
  "wifi": true,
  "smart_switching": false,
  "airplane_mode": false
}
```

## netconfig_get

Retrieves the current network configuration settings.

**Command format:** `netconfig_get`

**Arguments:** None

**Example:**
```
netconfig_get
```

**Response:** JSON object with current network configuration
```json
{
  "wifi": true,
  "smart_switching": true,
  "airplane_mode": false
}
```

## wipe_downloads

Deletes all files and directories in the downloads directory. This operation can be cancelled mid-execution.

Uses `/mnt/scratch/downloads` for diamond and `/mnt/updates/downloads` for pearl.

**Command format:** `wipe_downloads`

**Arguments:** None

**Example:**
```
wipe_downloads
```

**Response:** Status message with deletion statistics
```
Deleted 15, Failed 0
```

**Note:** If cancelled during execution, returns a partial count of items deleted before cancellation.

## service

Controls systemd services on the Orb (start, stop, restart, or check status).

**Command format:** `service <action> <service_name>`

**Arguments:**
- `action`: String - The action to perform. Must be one of: "start", "stop", "restart", "status"
- `service_name`: String - The name of the systemd service

**Security:** Service names are passed safely to systemctl without shell interpretation, preventing command injection attacks. Shell metacharacters (`;`, `|`, `&`, etc.) are treated as part of the service name and will cause systemctl to fail safely. This is forced by the way we invoke commands in the Shell trait.

**Examples:**
```
service stop worldcoin-core.service
service start orb-core.service
service restart orb-ui.service
service status worldcoin-core.service
```

**Response:** Output from systemctl command (e.g., service status information)

## change_name

Sets the Orb's device name by writing it to the configured orb name file path.

**Command format:** `change_name <orb-name>`

**Arguments:**
- `orb-name`: String - The new name for the Orb. Must contain a dash (e.g., "something-something")

**Example:**
```
change_name silly-philly
```

**Response:** Success message confirming the name was set
```
Orb name set to: silly-philly
```

## update_versions

Updates the Orb's versions.json file with a new version for the currently active slot. This handler automatically detects the current active slot (A or B) and updates the corresponding slot version in the versions file.

**Command format:** `update_versions <new_version>`

**Arguments:**
- `new_version`: String - The version string to set for the current slot (e.g., "v1.5.0")

**Behavior:**
- Automatically detects the current active slot using `orb-slot-ctrl -c`
- If versions.json exists and is valid, updates the appropriate slot version
- If versions.json is missing or invalid, creates a minimal structure with the new version for the current slot and "unknown" for the other slot
- Preserves existing version information for other components (jetson, mcu, etc.)

**Example:**
```
update_versions v1.5.0
```

**Response:** Success message with the updated versions.json content
```
Updated versions.json for slot_a
{
  "releases": {
    "slot_a": "v1.5.0",
    "slot_b": "v1.4.0"
  },
  "slot_a": {
    "jetson": {},
    "mcu": {}
  },
  "slot_b": {
    "jetson": {},
    "mcu": {}
  },
  "singles": {
    "jetson": {},
    "mcu": {}
  }
}
```

## slot_switch

Switches the Orb's boot slot and reboots.

**Command format:** `slot_switch <SlotSwitchArgs json>`

**Arguments:**
- `slot`: String - Target slot. Must be one of: "a", "b", or "other"
  - `"a"`: Switch to slot A
  - `"b"`: Switch to slot B
  - `"other"`: Switch to the opposite slot from the currently active one (automatically derived)

**Behavior:**
- Detects the current active slot using `orb-slot-ctrl -c`
- If target slot equals current slot, returns success with no action
- Otherwise, calls `sudo orb-slot-ctrl -s <target_slot>` to set the new slot
- Reboots the Orb using the `reboot` handler
- After reboot, completes the job successfully

**Note:** This is a sequential handler that blocks other jobs during execution.

**Examples:**
```
slot_switch {"slot":"a"}
slot_switch {"slot":"b"}
slot_switch {"slot":"other"}
```

**Response:**
- If already on target slot: Success with message indicating no action needed
- If switching slots: Progress update during switch/reboot, then success after reboot completes

**Progress messages:**
```
Switched from slot a to slot b, rebooting
rebooted
```

**Error cases:**
- Invalid slot argument (not "a", "b", or "other")
- `orb-slot-ctrl` command failures
- Reboot command failures
