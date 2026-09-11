# orb-backend-status

This daemon provide a dbus sink to collect status from varous orb systems and periodically provide them to the Orb Fleet Backend.

If you are on mac, be sure that you installed dbus. See the toplevel [README.md]

OES events may include an optional Unix-millisecond `created_at` in their Zenoh
attachment headers. The event envelope preserves that occurrence time through
forwarding and caching; legacy publishers without it use receipt time. This
support must precede core's buffered bootstrap and QR event rollout so delayed
events retain their original lifecycle ordering. See [OES](../oes/README.md).
