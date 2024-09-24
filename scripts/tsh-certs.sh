#!/usr/bin/env bash

set -o errexit   # abort on nonzero exit status
set -o errtrace  # pass ERR trap down to functions, substitutions, etc
set -o nounset   # abort on unbound variable
set -o pipefail  # don’t hide errors within pipes

# Function to display usage information
usage() {
    echo "Usage: $0 <teleport-tunnel> <destination-folder>

    Arguments:
      <teleport-tunnel>     Specify the teleport tunnel to use
      <destination-folder>  Specify the destination folder to copy files to
    "
}

main() {
    if [[ "$#" -ne 2 ]]; then
        usage
        exit 1
    fi

    local teleport_tunnel="${1}"
    local destination_folder="${2}"

    local -r files=(
        "70000001.extra.raw"
        "70000001.signature.raw"
        "70000001.pubkey.raw"
        "70000002.extra.raw"
        "70000002.signature.raw"
        "70000002.pubkey.raw"
        "7fff0206.chip_id.raw"
        "7fff0206.extra.raw"
        "7fff0206.signature.raw"
        "f0000013.cert"
        "sss_70000001_0002_0040.bin"
        "sss_70000002_0002_0040.bin"
        "sss_F0000012_0002_0040.bin"
        "sss_fat.bin"
    )

    # Create destination folder if it doesn't exist
    mkdir -p "${destination_folder}"

    # Loop through the files and use tsh scp to copy each one
    local file
    for file in "${files[@]}"; do
        echo "Copying ${file} from ${teleport_tunnel}..."
        if ! tsh scp "worldcoin@${teleport_tunnel}:/usr/persistent/se/keystore/${file}" "${destination_folder}/"; then
            echo "Error: Failed to copy ${file}"
        fi
    done
}

# Ensure that main only runs when called as a script
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    main "$@"
fi
