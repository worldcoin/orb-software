#!/usr/bin/env nu
# Allows running tests inside of podman.
# If nu shell is not there, install it: 'cargo install --locked nu'

# This test is supposed to run in 3 steps
# 1. Create a mockup directory: ./podman-runner.nu mock <dir>
# 2. Run the OTA on the mockup directory: ./podman-runner.nu run <path-to-update-agent> <dir>
# 3. Check that result of OTA is what is expected: ./podman-runner.nu check <dir>

# NOTE: only works if update-agent is built with 'cargo build --features skip-manifest-signature-verification'

use std log

def populate-mock-efivars [d] {
    0x[06 00 00 00 00 00 00 00] | save $"($d)/BootChainFwCurrent-781e084c-a330-417c-b678-38e696380cb9" --raw
    0x[07 00 00 00 00 00 00 00] | save $"($d)/RootfsStatusSlotB-781e084c-a330-417c-b678-38e696380cb9" --raw
    0x[06 00 00 00 03 00 00 00] | save $"($d)/RootfsRetryCountMax-781e084c-a330-417c-b678-38e696380cb9" --raw
    0x[07 00 00 00 03 00 00 00] | save $"($d)/RootfsRetryCountB-781e084c-a330-417c-b678-38e696380cb9" --raw
}

def populate-mock-usr-persistent [d] {
    cp -r mock-usr-persistent/* $d
}

# Create a squashfs partition with Linux fs. I would prefer to emulate orb-os
# more closely, but there is no publicly available images, so use fedora-bootc as closest approximation.
def populate-mnt-diamond [d] {
    podman run quay.io/fedora/fedora-bootc:latest tar --one-file-system -cf - . | mksquashfs - $"($d)/root.img" -tar -noappend -comp zstd
    let root_hash = cat $"($d)/root.img" | hash sha256
    let root_size = ls $"($d)/root.img" | get size.0 | into int

    echo  {
    "version": "6.3.0-LL-prod",
    "manifest": {
    "magic": "some magic",
    "type": "normal",
    "components": [
      {
        "name": "root",
        "version-assert": "none",
        "version": "none",
        "size": ($root_size),
        "hash": $"($root_hash)",
        "installation_phase": "normal"
      }
    ]
  },
  "manifest-sig": "TBD",
  "sources": {
    "root": {
      "hash": $"($root_hash)",
      "mime_type": "application/octet-stream",
      "name": "root",
      "size": $root_size,
      "url": "/mnt/root.img"
    },
  },
  "system_components": {
    "root": {
      "type": "gpt",
      "value": {
        "device": "emmc",
        "label": "ROOT",
        "redundancy": "redundant"
      }
    },
  }
  } | save $"($d)/claim.json"

  mkdir $"($d)/updates"
  return $d
}

def populate-mock-sd [sd] {

    truncate --size 64G $sd
    parted --script $sd mklabel gpt
    parted --script $sd mkpart APP_a 1M 65M
    parted --script $sd mkpart APP_b 65M 129M
    parted --script $sd mkpart esp 129M 193M
    parted --script $sd mkpart ROOT_a 193M 8385M
    parted --script $sd mkpart ROOT_b 8385M 16577M
    parted --script $sd mkpart persistent 16577M 16777M
    parted --script $sd mkpart MODELS_a 16777M 26777M
    parted --script $sd mkpart MODELS_b 26777M 36777M
}

def mock-systemctl [f] {
	["#!/bin/sh"
	 ""
	 "echo $@"] | save --force $f
	chmod +x $f
}

def cmp-xz-with-partition [ota_file, partition_img] {
    let res = (xzcat $ota_file | cmp $partition_img - | complete)

    if ( $res | get exit_code ) != 0 {
          log error "partition content does not match expected"
          log error ( $res | get stdout )
          log error ( $res | get stderr )
          return false
    }
    return true
}

def cmp-img-with-partition [ota_file, partition_img] {
    let sz = (ls $ota_file | get size.0 | into int)
    let res = (cmp --bytes=($sz) $ota_file $partition_img | complete)

    if ( $res | get exit_code ) != 0 {
          log error "partition content does not match expected"
          log error ( $res | get stdout )
          log error ( $res | get stderr )
          return false
    }
    return true
}

export def "main mock" [mock_path] {
    mkdir $mock_path
    mkdir $"($mock_path)/efivars"
    let mock_efivars = populate-mock-efivars $"($mock_path)/efivars"
    mkdir $"($mock_path)/usr_persistent"
    let mock_usr_persistent = populate-mock-usr-persistent $"($mock_path)/usr_persistent"
    let sd = populate-mock-sd $"($mock_path)/sd"
    mkdir $"($mock_path)/mnt"
    let mock_mnt = populate-mnt-diamond $"($mock_path)/mnt"
    let mock_mnt = mock-systemctl $"($mock_path)/systemctl"
}

def "main run" [prog, mock_path] {
    let absolute_path = ($prog | path expand)
    let mock_path = ($mock_path | path expand)
    mkdir /tmp/work
    mkdir /tmp/upper

    (podman run
     --rm
     -v $"($absolute_path):/var/mnt/program:Z"
     -w /var/mnt
     --security-opt=unmask=ALL
     $"--mount=type=bind,src=($mock_path)/efivars,dst=/sys/firmware/efi/efivars/,rw,relabel=shared,unbindable"
     --mount=type=bind,src=./orb_update_agent.conf,dst=/etc/orb_update_agent.conf,relabel=shared,ro
     --mount=type=bind,src=./os-release,dst=/etc/os-release,relabel=shared,ro
     $"--mount=type=bind,src=($mock_path)/usr_persistent,dst=/usr/persistent/,rw,relabel=shared"
     $"--mount=type=bind,src=($mock_path)/mnt,dst=/var/mnt,ro,relabel=shared"
     $"--mount=type=bind,src=($mock_path)/systemctl,dst=/usr/bin/systemctl,ro,relabel=shared"
     --mount=type=tmpfs,dst=/var/mnt/scratch/,rw
     $"--mount=type=bind,src=($mock_path)/sd,dst=/dev/mmcblk0,rw,relabel=shared"
     --volume="test:/sys/firmware:O,upperdir=/tmp/upper,workdir=/tmp/work"
     -e RUST_BACKTRACE
     -it quay.io/fedora/fedora-bootc:latest
     /var/mnt/program --nodbus
    )
}

def "main check" [mock_path] {
    let $sd = $"($mock_path)/sd"
    ["run"
    "download /dev/sda5 ./ROOT_b.after_ota.img"
    ] | str join "\n" | guestfish --rw -a $sd


    mut failed = false
    if not (cmp-img-with-partition $"($mock_path)/mnt/root.img" ./ROOT_b.after_ota.img) {
        log error "ROOT_b Test failed"
        $failed = true
    }
    rm ./ROOT_b.after_ota.img
    if $failed {
       exit 3
    }
}

export def "main clean" [mock_path] {
    rm -rf $mock_path
}

# Integration testing of update agent
def main [] {
  echo "main"
}
