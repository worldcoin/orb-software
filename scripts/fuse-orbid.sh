#!/bin/bash

set -o errexit   # abort on nonzero exit status
set -o errtrace  # pass ERR trap down to functions, substitutions, etc
set -o nounset   # abort on unbound variable
set -o pipefail  # don’t hide errors within pipes

main () {
    # Ensure privileged execution
    if [[ $(id -u) -ne 0 ]]; then
        echo "This script must be run as root" >&2
        exit 1
    fi

    orb_id=$(dd if=/dev/disk/by-partlabel/UID-PUB bs=512 count=1 status=none | \
         sed 's/\x00*$//' | cut -d' ' -f2 | tr -d '\n' | sha256sum | cut -c1-8)

    echo "0x${orb_id}" > /sys/devices/platform/tegra-fuse/reserved_odm0
    echo "0x1" > /sys/devices/platform/tegra-fuse/odm_lock
}

# Ensure that main only runs when called as a script
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    main "$@"
fi

