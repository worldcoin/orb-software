#!/usr/bin/env nu
# Allows running tests inside of podman.
# If nu shell is not there, install it: 'cargo install --locked nu'

## TODO how to cleanup the temp directory?
def populate-mock-efivars [] {
	let d = (mktemp --directory)
	0x[06 00 00 00 00 00 00 00] | save $"($d)/BootChainFwCurrent-781e084c-a330-417c-b678-38e696380cb9" --raw
	0x[07 00 00 00 00 00 00 00] | save $"($d)/RootfsStatusSlotB-781e084c-a330-417c-b678-38e696380cb9" --raw

	return $d
}

## TODO how to cleanup the temp directory?
def populate-mock-usr-persistent [] {
	let d = (mktemp --directory)
	cp -r mock-usr-persistent/* $d
	return $d
}


def populate-mock-mmcblk [] {
	#TODO cleanup at the END not in the beginning
	rm mmcblk0
	# Disk /dev/mmcblk0: 14.69 GiB, 15758000128 bytes, 30777344 sectors
	truncate --size 15758000128 mmcblk0

	parted --script mmcblk0 mklabel gpt
	parted --script mmcblk0 mkpart primary 40s 131111s
	parted --script mmcblk0 name 1 APP_a
	parted --script mmcblk0 mkpart primary 131112s 262183s
	parted --script mmcblk0 name 2 APP_b

	# TODO actually reproduce orb partition table
	parted --script mmcblk0 mkpart primary 262184s 462183s
	parted --script mmcblk0 name 3 CACHE_LAYER_b
	parted --script mmcblk0 mkpart primary 462184s 662183s
	parted --script mmcblk0 name 4 SOFTWARE_LAYER_b
	parted --script mmcblk0 mkpart primary 662184s 862183s
	parted --script mmcblk0 name 5 SYSTEM_LAYER_b

	return mmcblk0
}
# NOTE: only works if built with 'cargo build --features skip-manifest-signature-verification'

def main [prog, args] {
	let absolute_path = ($prog | path expand)

	let mock_efivars = populate-mock-efivars
	let mock_usr_persistent = populate-mock-usr-persistent
	let mmcblk0 = populate-mock-mmcblk

	# TODO add overlay for persistent
	(podman run
	 --rm
	 -v $"($absolute_path):/mnt/program:Z"
	 -w /mnt
	 --security-opt=unmask=/sys/firmware
	 --security-opt=mask=/sys/firmware/acpi:/sys/firmware/dmi:/sys/firmware/memmap
	 --mount=type=bind,src=($mock_efivars),dst=/sys/firmware/efi/efivars/,rw,relabel=shared,unbindable
	 --mount=type=bind,src=./orb_update_agent.conf,dst=/etc/orb_update_agent.conf,relabel=shared,ro
	 --mount=type=bind,src=($mock_usr_persistent),dst=/usr/persistent/,rw,relabel=shared
	 --mount=type=bind,src=./claim.json,dst=/mnt/claim.json,ro,relabel=shared
	 --mount=type=bind,src=./s3_bucket,dst=/mnt/s3_bucket/,ro,relabel=shared
	 --mount=type=tmpfs,dst=/mnt/updates/
	 --mount=type=bind,src=($mmcblk0),dst=/dev/mmcblk0,rw,relabel=shared
	 -e RUST_BACKTRACE
	 -it fedora:latest
	 /mnt/program --nodbus
	)

#
#	 -v $"($mock_efivars):/tmp/firmware/:O"
#	 --mount=type=bind,src=($mock_efivars),dst=/sys/firmware/,ro,relabel=shared,unbindable

}
