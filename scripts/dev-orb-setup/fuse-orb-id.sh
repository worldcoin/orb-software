#!/usr/bin/env bash

set -o errexit
set -o errtrace
set -o nounset
set -o pipefail

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" &> /dev/null && pwd)
BASE_DIR=$(cd -- "$(dirname -- "${SCRIPT_DIR}")" &> /dev/null && pwd)

usage() {
    echo "Description: Script to fuse a random ORB-ID"
    echo "Usage: $0 <orb-ip-address> [-s <ssh-socket-name>]"
    echo ""
    echo "Arguments:"
    echo "  <orb-ip-address>              The IP address of the orb."
    echo "  -s | --ssh-socket <name>      Optional SSH socket name."
}


main() {
    local arg
    local positional_args=()
    local ssh_socket="tmp-ssh-socket"

    while [[ $# -gt 0 ]]; do
        arg="${1}"; shift
        case ${arg} in
            -h | --help)
                usage
                exit 0
                ;;
            -s | --ssh-socket)
                ssh_socket="${1}"; shift
                ;;
            -*)
                echo "Invalid argument: ${arg}"
                usage
                exit 1
                ;;
            *)
                positional_args+=("${arg}")
                ;;
        esac
    done

    set -- "${positional_args[@]}"
    if [[ $# -ne 1 ]]; then
        echo "Error: Exactly one positional argument is required."
        usage
        exit 1
    fi

    local orb_ip="${1}"
    local lock_status
    local orb_id

    ssh -M -S "${ssh_socket}" -fN worldcoin@"${orb_ip}"

    lock_status="$(ssh -S "${ssh_socket}" worldcoin@"${orb_ip}" 'cat /sys/devices/platform/tegra-fuse/odm_lock')"

    if [[ "${lock_status}" == "0x00000001" ]]; then
        echo "orb-id is already fused."
        ssh -S "${ssh_socket}" -O exit worldcoin@"${orb_ip}"
        exit 0
    fi

    orb_id="$(openssl rand -hex 4)"
    echo "Generated random orb-id = ${orb_id}"

    ssh -S "${ssh_socket}" worldcoin@"${orb_ip}" "sudo sh -c 'echo \"0x${orb_id}\" > /sys/devices/platform/tegra-fuse/reserved_odm0'"
    ssh -S "${ssh_socket}" worldcoin@"${orb_ip}" "sudo sh -c 'echo \"0x1\" > /sys/devices/platform/tegra-fuse/odm_lock'"
    ssh -S "${ssh_socket}" -O exit worldcoin@"${orb_ip}"
    exit 0
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    main "$@"
fi
