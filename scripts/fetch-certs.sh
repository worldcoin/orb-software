#!/usr/bin/env bash

set -o errexit   # abort on nonzero exit status
set -o errtrace  # pass ERR trap down to functions, substitutions, etc
set -o nounset   # abort on unbound variable
set -o pipefail  # don’t hide errors within pipes

# Function to display usage information
usage() {
    echo "Usage: $0 [OPTIONS] <teleport-tunnel|ipv4> <destination-folder>

    Arguments:
        <teleport-tunnel|ipv4>      Specify the teleport tunnel or IPv4 address
        <destination-folder>        Specify the destination folder for fetching

    Options:
       -s, --short                  Skip the attestation certificate
       -p, --passphrase             Expected to be used with IPv4 address

    Description:
        Copy the certificates from the device to the host machine.
    Notes:
        For development images, both the teleport tunnel and IPv4 address can be used.
        For production images, the ssh client is disabled, only teleport can by used."
}

main() {
    local arg
    local remote
    local short=false
    local passphrase=""
    local scp_prefix="tsh"
    local positional_args=()
    while [[ $# -gt 0 ]]; do
        arg="${1}"; shift
        case ${arg} in
            -h | --help)
                usage; exit 0 ;;
            -s | --short)
                short=true ;;
            -p | --passphrase)
                passphrase=${1}; shift ;;
            -*)
                echo "invalid argument: ${arg}"
                usage; exit 1 ;;
            *)
                positional_args+=( "${arg}" ) ;;
        esac
    done
    set -- "${positional_args[@]}"

    if [[ "$#" -ne 2 ]]; then
        echo "Error: Arguments are missing"
        usage; exit 1
    fi

    remote="${1}"
    if [[ "${remote}" =~ ^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
        if [[ -z "${passphrase}" ]]; then
            echo "Error: Passphrase is missing"
            usage; exit 1
        fi
        scp_prefix="sshpass -p "${passphrase}""
    fi

    local destination_folder="${2}"
    mkdir -p "${destination_folder}"

    local -a certificates=(
        "70000001.extra.raw"
        "70000001.signature.raw"
        "70000001.pubkey.raw"
        "70000002.extra.raw"
        "70000002.signature.raw"
        "70000002.pubkey.raw"
        "7fff0206.chip_id.raw"
        "7fff0206.extra.raw"
        "7fff0206.signature.raw"
        "sss_70000001_0002_0040.bin"
        "sss_70000002_0002_0040.bin"
        "sss_F0000012_0002_0040.bin"
        "sss_fat.bin"
    )
    local -r attestation_cert="f0000013.cert"
    if [[ "${short}" == false ]]; then
        certificates+=( "${attestation_cert}" )
    fi

    local file
    for file in "${certificates[@]}"; do
        if [[ "$short" == true && "$file" == "f0000013.cert" ]]; then
            continue
        fi
        echo "Copying ${file} from ${remote}..."
        if ! ${scp_prefix} scp "worldcoin@${remote}:/usr/persistent/se/keystore/${file}" "${destination_folder}/"; then
            echo "Error: Failed to copy ${file}"
        fi
    done
}

# Ensure that main only runs when called as a script
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    main "$@"
fi
