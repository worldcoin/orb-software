# orb-slot-ctrl

The Slot Control is a tool to read and write the slot and rootfs state of the Orb.

## Command line arguments

For available command line arguments see `slot-ctrl --help`.
Those are the high level commands:

```sh
Usage: slot-ctrl <COMMAND>

Commands:
  current, -c  Get the current active slot
  next, -n     Get the slot set for the next boot
  set, -s      Set slot for the next boot
  status       Rootfs status controls
  git, -g      Get the git commit used for this build
  help         Print this message or the help of the given subcommand(s)
```

And here are the subcommands for `status`:

```sh
Usage: slot-ctrl status [OPTIONS] <COMMAND>

Commands:
  get, -g      Get the rootfs status
  set, -s      Set the rootfs status
  retries, -c  Get the retry counter
  reset, -r    Set the retry counter to maximum
  max, -m      Get the maximum retry counter
  list, -l     Get a full list of rootfs status variants
  help         Print this message or the help of the given subcommand(s)

Options:
  -i, --inactive  Control the inactive slot instead of the active
```

## Platform support

Code builds on both linux and macos, but it only runs on the
orb.
