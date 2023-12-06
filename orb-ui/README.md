# orb-ui

This binary is responsible for running the UI on the orb.

Orb states and events are communicated to the UI via a DBus interface.
LEDs are controlled by sending all RGB LEDs values to the main
microcontroller via a serial port, at 60fps.

