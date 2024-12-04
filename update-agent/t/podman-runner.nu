#!/usr/bin/env nu
# Allows running tests inside of podman.
# If nu shell is not there, install it: 'cargo install --locked nu'

## TODO how to cleanup the temp directory?
def populate-mock-efivars [] {
	let d = (mktemp --directory)
	let slot_a = 0x[06 00 00 00 00 00 00 00]
	$slot_a | save $"($d)/BootChainFwCurrent-781e084c-a330-417c-b678-38e696380cb9" --raw
	return $d
}

def main [prog, args] {
	let absolute_path = ($prog | path expand)

	let mock_efivars = populate-mock-efivars

	(podman run
	 --rm
	 -v $"($absolute_path):/mnt/program:Z"
	 -w /mnt
	 --security-opt=unmask=/sys/firmware
	 --security-opt=mask=/sys/firmware/acpi:/sys/firmware/dmi:/sys/firmware/memmap
	 --mount=type=bind,src=($mock_efivars),dst=/sys/firmware/efi/efivars/,ro,relabel=shared,unbindable
	 -e RUST_BACKTRACE
	 -it fedora:latest)

#
#	 -v $"($mock_efivars):/tmp/firmware/:O"
#	 --mount=type=bind,src=($mock_efivars),dst=/sys/firmware/,ro,relabel=shared,unbindable

}
