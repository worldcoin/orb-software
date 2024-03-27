# CHANGELOG

## v0.3.0

Move to orb-software repo. This is the first release since the original
author left.

## v0.2.4

No notes recorded :(

## v0.2.3

New patch release of Slot Control binary and library.

Changes:

- only require root privileges in the binary for set commands
- remove the file attribute restore on Drop of efivar

## v0.2.2

New patch release of Slot Control binary and library.
Only a few usability adjustments and a fix for inconsitent Slot format when printing.

Changes:

- Add additional get slot exe paths (just match on file name now)
- Remove debug format for Slot to fix incositencies

## v0.2.1

New patch release of Slot Control binary and library.
Only a few usability adjustments

Changes:

- Install a `get-slot` symlink with the debian package
- If executable name is `get-slot`, `slot-ctrl` binary now returns the current slot
- Change Slot display format to lowercase to match Nvidia standards

## v0.2.0

New release of Slot Control binary and library.
This update includes changes in efivar manipulation necessary for jetson linux 35.2.1.

Efivar manipulation needs to be in sync with the bootloader implementation, which can be found here.
Otherwise this tool can cause the Orb to fail booting.

Changes:

Adjust efivar manipultaion to match jetson linux 35.2.1 implementation
- Adjust libary functions slighly
- Update tests
- Update docs
- Add new Errors CreateFile and ExceedingRetryCount
- Rename Error InvalidCurrentSlotData to InvalidSlotData to be more
- generic
- Add new Efivar function to create a new efivar on demand which does
- not need to set any file attributes
- Remove println for set functions

## v0.1.0

Initial release of Slot Control binary and library.
It's a tool designed to read and write the slot and rootfs state of the Orb.
Efivar manipulation is implemented for efivars of jetson linux 35.1.

Efivar manipulation needs to be in sync with the bootloader implementation, which can be found here.
Otherwise this tool can cause the Orb to fail booting.

Features:

- Get the current active slot
- Get the slot set for the next boot
- Set slot for the next boot
- Get the rootfs status for active and inactive
- Set the rootfs status for active and inactive slot
- Get the retry counter for active and inactive slot
- Set the retry counter to maximum for active and inactive slot
- Get the maximum retry counter
