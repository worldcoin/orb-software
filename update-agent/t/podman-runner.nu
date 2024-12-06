#!/usr/bin/env nu
# Allows running tests inside of podman.
# If nu shell is not there, install it: 'cargo install --locked nu'

## TODO how to cleanup the temp directory?
def populate-mock-efivars [] {
	let d = (mktemp --directory)
	0x[06 00 00 00 00 00 00 00] | save $"($d)/BootChainFwCurrent-781e084c-a330-417c-b678-38e696380cb9" --raw
	0x[07 00 00 00 00 00 00 00] | save $"($d)/RootfsStatusSlotB-781e084c-a330-417c-b678-38e696380cb9" --raw
	0x[06 00 00 00 03 00 00 00] | save $"($d)/RootfsRetryCountMax-781e084c-a330-417c-b678-38e696380cb9" --raw
	0x[07 00 00 00 03 00 00 00] | save $"($d)/RootfsRetryCountB-781e084c-a330-417c-b678-38e696380cb9" --raw

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

# root@localhost:~# parted /dev/mmcblk0
# uGNU Parted 3.3
# Using /dev/mmcblk0
# Welcome to GNU Parted! Type 'help' to view a list of commands.
# (parted) unit B print
# Model: MMC DG4016 (sd/mmc)
# Disk /dev/mmcblk0: 15758000128B
# Sector size (logical/physical): 512B/512B
# Partition Table: gpt
# Disk Flags:

# Number  Start         End           Size         File system  Name                  Flags
#  1      20480B        67129343B     67108864B    fat16        APP_a                 msftdata
#  2      67129344B     134238207B    67108864B    fat16        APP_b                 msftdata
#  3      134238208B    186667007B    52428800B                 BASE_LAYER_a          msftdata
#  4      186667008B    710955007B    524288000B                LFT_LAYER_a           msftdata
#  5      710955008B    1497387007B   786432000B                PACKAGES_LAYER_a      msftdata
#  6      1497387008B   5255483391B   3758096384B               CUDA_LAYER_a          msftdata
#  7      5255483392B   5360340991B   104857600B                SYSTEM_LAYER_a        msftdata
#  8      5360340992B   5361389567B   1048576B                  SECURITY_LAYER_a      msftdata
#  9      5361389568B   7240437759B   1879048192B               AI_LAYER_a            msftdata
# 10      7240437760B   7307546623B   67108864B                 SOFTWARE_LAYER_a      msftdata
# 11      7307546624B   7308595199B   1048576B                  CACHE_LAYER_a         msftdata
# 12      7308595200B   7361023999B   52428800B                 BASE_LAYER_b          msftdata
# 13      7361024000B   7885311999B   524288000B                LFT_LAYER_b           msftdata
# 14      7885312000B   8671743999B   786432000B                PACKAGES_LAYER_b      msftdata
# 15      8671744000B   12429840383B  3758096384B               CUDA_LAYER_b          msftdata
# 16      12429840384B  12534697983B  104857600B                SYSTEM_LAYER_b        msftdata
# 17      12534697984B  12535746559B  1048576B                  SECURITY_LAYER_b      msftdata
# 18      12535746560B  14414794751B  1879048192B               AI_LAYER_b            msftdata
# 19      14414794752B  14481903615B  67108864B                 SOFTWARE_LAYER_b      msftdata
# 20      14481903616B  14482952191B  1048576B                  CACHE_LAYER_b         msftdata
# 21      15134097408B  15136718847B  2621440B                  secure-os_b           msftdata
# 22      15136718848B  15136784383B  65536B                    eks_b                 msftdata
# 23      15136784384B  15137832959B  1048576B                  adsp-fw_b             msftdata
# 24      15137832960B  15138881535B  1048576B                  rce-fw_b              msftdata
# 25      15138881536B  15139930111B  1048576B                  sce-fw_b              msftdata
# 26      15139930112B  15141502975B  1572864B                  bpmp-fw_b             msftdata
# 27      15141502976B  15142551551B  1048576B                  bpmp-fw-dtb_b         msftdata
# 28      15142551552B  15209660415B  67108864B    fat32        esp                   boot, esp
# 29      15301738496B  15301758975B  20480B                    spacer                msftdata
# 30      15301758976B  15367819263B  66060288B                 recovery              msftdata
# 31      15367819264B  15368343551B  524288B                   recovery-dtb          msftdata
# 32      15368343552B  15368605695B  262144B                   kernel-bootctrl       msftdata
# 33      15368605696B  15368867839B  262144B                   kernel-bootctrl_b     msftdata
# 34      15368867840B  15683440639B  314572800B                RECROOTFS             msftdata
# 35      15683440640B  15683441663B  1024B                     UID                   msftdata
# 36      15683441664B  15683442687B  1024B                     UID-PUB               msftdata
# 37      15683442688B  15684491263B  1048576B     ext2         PERSISTENT            msftdata
# 38      15684491264B  15694977023B  10485760B    ext4         PERSISTENT-JOURNALED  msftdata
# 39      15694977024B  15757983231B  63006208B                 UDA                   msftdata

	truncate --size 15758000128 mmcblk0
	parted --script mmcblk0 mklabel gpt
	parted --script mmcblk0 mkpart primary 20480B        67129343B
	parted --script mmcblk0 name 1    APP_a
	parted --script mmcblk0 mkpart primary 67129344B     134238207B
	parted --script mmcblk0 name 2    APP_b
	parted --script mmcblk0 mkpart primary 134238208B    186667007B
	parted --script mmcblk0 name 3    BASE_LAYER_a
	parted --script mmcblk0 mkpart primary 186667008B    710955007B
	parted --script mmcblk0 name 4   LFT_LAYER_a
	parted --script mmcblk0 mkpart primary 710955008B    1497387007B
	parted --script mmcblk0 name 5   PACKAGES_LAYER_a
	parted --script mmcblk0 mkpart primary 1497387008B   5255483391B
	parted --script mmcblk0 name 6  CUDA_LAYER_a
	parted --script mmcblk0 mkpart primary 5255483392B   5360340991B
	parted --script mmcblk0 name 7   SYSTEM_LAYER_a
	parted --script mmcblk0 mkpart primary 5360340992B   5361389567B
	parted --script mmcblk0 name 8     SECURITY_LAYER_a
	parted --script mmcblk0 mkpart primary 5361389568B   7240437759B
	parted --script mmcblk0 name 9  AI_LAYER_a
	parted --script mmcblk0 mkpart primary 7240437760B   7307546623B
	parted --script mmcblk0 name 10    SOFTWARE_LAYER_a
	parted --script mmcblk0 mkpart primary 7307546624B   7308595199B
	parted --script mmcblk0 name 11     CACHE_LAYER_a
	parted --script mmcblk0 mkpart primary 7308595200B   7361023999B
	parted --script mmcblk0 name 12    BASE_LAYER_b
	parted --script mmcblk0 mkpart primary 7361024000B   7885311999B
	parted --script mmcblk0 name 13   LFT_LAYER_b
	parted --script mmcblk0 mkpart primary 7885312000B   8671743999B
	parted --script mmcblk0 name 14   PACKAGES_LAYER_b
	parted --script mmcblk0 mkpart primary 8671744000B   12429840383B
	parted --script mmcblk0 name 15  CUDA_LAYER_b
	parted --script mmcblk0 mkpart primary 12429840384B  12534697983B
	parted --script mmcblk0 name 16   SYSTEM_LAYER_b
	parted --script mmcblk0 mkpart primary 12534697984B  12535746559B
	parted --script mmcblk0 name 17     SECURITY_LAYER_b
	parted --script mmcblk0 mkpart primary 12535746560B  14414794751B
	parted --script mmcblk0 name 18  AI_LAYER_b
	parted --script mmcblk0 mkpart primary 14414794752B  14481903615B
	parted --script mmcblk0 name 19    SOFTWARE_LAYER_b
	parted --script mmcblk0 mkpart primary 14481903616B  14482952191B
	parted --script mmcblk0 name 20     CACHE_LAYER_b
	parted --script mmcblk0 mkpart primary 15134097408B  15136718847B
	parted --script mmcblk0 name 21     secure-os_b
	parted --script mmcblk0 mkpart primary 15136718848B  15136784383B
	parted --script mmcblk0 name 22       eks_b
	parted --script mmcblk0 mkpart primary 15136784384B  15137832959B
	parted --script mmcblk0 name 23     adsp-fw_b
	parted --script mmcblk0 mkpart primary 15137832960B  15138881535B
	parted --script mmcblk0 name 24     rce-fw_b
	parted --script mmcblk0 mkpart primary 15138881536B  15139930111B
	parted --script mmcblk0 name 25     sce-fw_b
	parted --script mmcblk0 mkpart primary 15139930112B  15141502975B
	parted --script mmcblk0 name 26     bpmp-fw_b
	parted --script mmcblk0 mkpart primary 15141502976B  15142551551B
	parted --script mmcblk0 name 27     bpmp-fw-dtb_b
	parted --script mmcblk0 mkpart primary 15142551552B  15209660415B
	parted --script mmcblk0 name 28    esp
	parted --script mmcblk0 mkpart primary 15301738496B  15301758975B
	parted --script mmcblk0 name 29       spacer
	parted --script mmcblk0 mkpart primary 15301758976B  15367819263B
	parted --script mmcblk0 name 30    recovery
	parted --script mmcblk0 mkpart primary 15367819264B  15368343551B
	parted --script mmcblk0 name 31      recovery-dtb
	parted --script mmcblk0 mkpart primary 15368343552B  15368605695B
	parted --script mmcblk0 name 32      kernel-bootctrl
	parted --script mmcblk0 mkpart primary 15368605696B  15368867839B
	parted --script mmcblk0 name 33      kernel-bootctrl_b
	parted --script mmcblk0 mkpart primary 15368867840B  15683440639B
	parted --script mmcblk0 name 34   RECROOTFS
	parted --script mmcblk0 mkpart primary 15683440640B  15683441663B
	parted --script mmcblk0 name 35        UID
	parted --script mmcblk0 mkpart primary 15683441664B  15683442687B
	parted --script mmcblk0 name 36        UID-PUB
	parted --script mmcblk0 mkpart primary 15683442688B  15684491263B
	parted --script mmcblk0 name 37     PERSISTENT
	parted --script mmcblk0 mkpart primary 15684491264B  15694977023B
	parted --script mmcblk0 name 38    PERSISTENT-JOURNALED
	parted --script mmcblk0 mkpart primary 15694977024B  15757983231B
	parted --script mmcblk0 name 39    UDA

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
