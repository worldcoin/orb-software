# orb-ui

This binary is responsible for running the UI on the orb.

Orb states and events are communicated to the UI via a DBus interface.
LEDs are controlled by sending all RGB LEDs values to the main
microcontroller via a serial port, at 60fps.

## Commands

Orb UI daemon

Usage: orb-ui <COMMAND>

Commands:
  daemon      Orb UI daemon, listening and reacting to dbus messages
  simulation  Signup simulation
  recovery    Recovery UI
  help        Print this message or the help of the given subcommand(s)

## Daemon

Test new event with the orb-ui daemon running:

```shell
busctl --user call org.worldcoin.OrbUiState1 /org/worldcoin/OrbUiState1 org.worldcoin.OrbUiState1 OrbUiStateEvent s "\"Bootup\""
```

## Platform Support

Compiles and runs on both linux and macOS.

## Tokio Console Support

Supported. See the [toplevel README](../README.md) for info on how to use it.
