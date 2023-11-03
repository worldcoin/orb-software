# orb-backend-state
This daemon retrieves the orb's state from the orb-manager backend service, and exposes
that for other services to read, via dbus.

If you are on mac, be sure that you installed dbus. See the toplevel [README.md]

Note: This service is intentionally dumb, and stringly typed. This is because its only
responsibility is to act as a proxy for data - the daemon doesn't care at all about
the representation of that data.

## Env Vars
- `ORB_AUTH_TOKEN` - optional, provide this to manually set the auth token instead of using the short lived token daemon
- `ORB_ID` - optional, provide this to set the orb id instead of calling the orb-id binary.

## Busctl
You can easily interact with dbus services with busctl:
```
busctl call --address=unix:path=/tmp/worldcoin_bus_socket org.worldcoin.BackendState /org/worldcoin/BackendState org.freedesktop.DBus.Properties Get ss "org.worldcoin.BackendState1" "State"

busctl call --address=unix:path=/tmp/worldcoin_bus_socket org.worldcoin.BackendState /org/worldcoin/BackendState org.worldcoin.BackendState1 RefreshState
```
