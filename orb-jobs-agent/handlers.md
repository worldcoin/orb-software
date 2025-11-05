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
- `sec`: String - Security type, one of the two supported WifiSec enum variants: "Wpa2Psk" or "Wpa3Sae"
- `pwd`: String - Password for the network (must be at least 8 characters)
- `hidden`: Boolean (optional, default: false) - Whether the network is hidden
- `join_now`: Boolean (optional, default: false) - Whether to attempt to connect to the network immediately after adding the profile

**Example:**
```
wifi_add {"ssid":"HomeWIFI","sec":"Wpa2Psk","pwd":"12345678","hidden":false,"join_now":true}
```

**Response:** JSON object indicating connection status
```json
{"connection_success": true}  // or false if connection failed, or null if join_now was false
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

**Response:** Success status with no additional output

## wifi_list

Lists all saved WiFi network profiles.

**Command format:** `wifi_list`

**Arguments:** None

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
    "psk": "unencrypted_password"
  }
]
```

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

