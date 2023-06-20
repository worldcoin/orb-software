# Changelog

## 0.4.0

### Added

+ Proxy for logind method `org.freedesktop.login1.Manager.ScheduleShutdown`
    + enables `orb-core` and `update-agent` to shutdown or restart the device without
      needing to grant elevated priveleges/suid

## 0.3.0

`orb-supervisor` no longer shuts down `orb-core` immediately when an update happens
but waits until no new signups have been started for a while.

### Added

+ Upon receiving a `RequestUpdatePermission` request, `orb-supervisor` only shuts
  down `orb-core` after 20 minutes of inactivity (meaning that no signups have been
  performed for 20 minutes). This timer is reset every time a new signup starts.
  Once the timer is up, `orb-supervisor` schedules `update-agent` to immediately run again.

### Changed

+ `orb-supervisor` now returns custom `MethodError`s to report why an update was denied,
  bringing it more in line with DBus conventions. 

## 0.2.0 (October 20, 2022)

`orb-supervisor`'s integration with systemd and journald is improved by using
journald conventions and writing directly to the journald socket.

### Added

+ `orb-supervisor` detects if its attached to an interactive TTY using `STDIN`:
    + if not attached to a TTY, it will write to the journald socket
    + if attached to a TTY, it will write to stdout/stderr
+ `orb-supervisor` identifies itself as `worldcoin-supervisor` using SYSLOG IDENT;
    + use `journalctl -t worldcoin-supervisor` to filter journald entries
      (`-u worldcoin-supervisor` however is still the preferred way);

## 0.1.0 (August 31, 2022)

This is the first release of `orb-supervisor`.

### Added

+ Expose dbus property `org.worldcoin.OrbSupervisor1.Manager.BackgroundDownloadsAllowed`;
    + Tracks how much time has passed since the last
    `org.worldcoin.OrbCore1.Signup.SignupStarted` events;
+ Expose dbus method `org.worldcoin.OrbSupervisor1.Manager.RequestUpdatePermission`;
    + attempts to shutdown `worldcoin-core.service` through
    `org.freedesktop.systemd1.Manager.StopUnit`;
