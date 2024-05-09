# orb-wpa-supplicant

Provides a replacement for wpa_cli, by using wpa supplicant's DBUS interface.

This crate is currently unused, because talking to wpa_supplicant requires root.
This is too risky, so once we configure polkit, we will begin using this crate.

## Platform support notes

This binary builds on all targets but assumes the presence of the
wpa-supplicant daemon and the wlan0 network interface, so it is unlikely to run outside of the orb.
