#!/usr/bin/env bash

set -o errexit   # abort on nonzero exitstatus
set -o nounset   # abort on unbound variable
set -o pipefail  # don't hide errors within pipes

# Checks that the sounds directory only includes *.wav files
# Checks that the sounds are sampled on 16 bits
# Guards against bad archive preparation.

validate_sounds::validate() {
    local -r sounds_dir=${1}

    pushd "${sounds_dir}" >/dev/null

    local check_bit_sampling=true
    if ! [[ -x "$(command -v soxi)" ]]; then
        echo "Install sox (soxi) to check bits per sample" >>/dev/stderr
        check_bit_sampling=false
    fi

    for file in *; do
        if ! [[ "${file}" =~ \.wav$ ]] ; then
            echo "Invalid sounds directory: '${file}' is not a wav file"
            exit 1
        fi

        if [[ "${check_bit_sampling}" = true ]]; then
            local -i bits_per_sample
            bits_per_sample="$(soxi -b "${file}")"

            if [[ "${bits_per_sample}" != 16 ]]; then
                echo "${file} must be converted to 16 bits per sample"
                exit 1
            fi
        fi
    done

    popd >/dev/null

    echo "Sounds directory successfully validated."
}

main() {
    trap trap_err ERR

    validate_sounds::validate "${1}"
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    main "$@"
fi
