# Changelog

## 0.1.0 (August 31, 2022)

This is the first release of `orb-supervisor`.

### Added

+ Expose dbus property `org.worldcoin.OrbSupervisor1.Manager.BackgroundDownloadsAllowed`;
    + Tracks how much time has passed since the last
    `org.worldcoin.OrbCore1.Signup.SignupStarted` events;
+ Expose dbus method `org.worldcoin.OrbSupervisor1.Manager.RequestUpdatePermission`;
    + attempts to shutdown `worldcoin-core.service` through
    `org.freedesktop.systemd1.Manager.StopUnit`;
