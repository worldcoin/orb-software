#!/usr/bin/env bash

set -o errexit   # abort on nonzero exit status
set -o errtrace  # pass ERR trap down to functions, substitutions, etc
set -o nounset   # abort on unbound variable
set -o pipefail  # don’t hide errors within pipes

[[ "${BASH_VERSINFO:-0}" -ge 4 ]] || { echo "Bash version 4 or higher is required."; exit 1; }

usage() {
    echo "Usage $0 [OPTIONS] <teleport-tunnel|ipv4>

    Options:
        -h, --help          Print help
        -r, --reprovision   Re-provisioning the device and skips fetching of attestation certificate
        -p, --passphrase    Expected to be used with IPv4 (required for dev images with ssh access)
        -t, --plug-trust    Path for plug_and_trust.tar.gz on the host (required for prod images).
                            Check the worldcoin/plug-and-trust/releases for the appropriate version
                            based on the device's software release
        -y, --assumeyes     automatically answer yes for all questions

    Example:
        $0 -r -t ~/Downloads/plug_and_trust.tar.gz <teleport-tunnel>"
}

provision_device() {
    local remote="${1}"
    local reprovision="${2}"
    local ssh_prefix="${3}"
    local plug_trust="${4}"
    local interactive="${5}"

    local user="worldcoin"
    local se_dir="/usr/persistent/se"
    local key_dir="${se_dir}/keystore"
    if [[ "${ssh_prefix}" == "tsh" ]]; then
        user="root"
    fi

    # If /se/keystore is not present, provisioning process was never executed, or keystore was wiped
    # In this case, reprovisioniong is not allowed for fear of wiping the attestation certificate
    if [[ ${reprovision} ]]; then
        if [[ ${interactive} ]]; then
            read -p "Reprovisioning will wipe all provisioning material, continue? [y/N] " -n 1 -r
            echo
            if [[ ! ${REPLY} =~ ^[Yy]$ ]]; then
                echo "Reprovisioning aborted"
                exit 0
            fi
        fi
        ${ssh_prefix} ssh "${user}@${remote}" bash --noprofile --norc <<EOF
        set -euo
        if ! [[ -d ${key_dir} ]]; then
            echo '${key_dir} does not exist, re-provisioning blocked'
            exit 1
        fi
        cp /${key_dir}/f0000013.cert /usr/persistent/ || true
        sudo su || true
        mount -o remount,exec /tmp
        systemctl stop nv-tee-supplicant.service
        rm -rf /usr/persistent/tee/ ${key_dir}
        systemctl start nv-tee-supplicant.service
        su worldcoin -c 'cd -- ${se_dir}; \
            /${plug_trust}/delete-all.sh || true; /${plug_trust}/provision.sh --short'
        cp /usr/persistent/f0000013.cert ${key_dir} || true
        exit 0
EOF
    else
        ${ssh_prefix} ssh worldcoin@"${remote}" bash --noprofile --norc <<EOF
        set -euo
        mkdir -p ${se_dir}
        cd -- ${se_dir}
        /service_mode/provision.sh
EOF
    fi
}

main() {
    local arg
    local remote
    local plug_trust=""
    local reprovision=false
    local passphrase=""
    local ssh_prefix="tsh"
    local interactive=true
    local positional_args=()

    while [[ $# -gt 0 ]]; do
        arg="${1}"; shift
        case ${arg} in
            -h | --help)
                usage; exit 0 ;;
            -r | --reprovision)
                reprovision=true ;;
            -p | --passphrase)
                passphrase="${1}"; shift ;;
            -t | --plug-trust)
                plug_trust="${1}"; shift ;;
            -y | --assumeyes)
                interactive=false ;;
            -*)
                echo "Invalid argument: ${arg}"
                usage; exit 1 ;;
            *)
                positional_args+=( "${arg}" ) ;;
        esac
    done
    set -- "${positional_args[@]}"

    if [[ "$#" -ne 1 ]]; then
        echo "Error: teleport-tunnel or IPv4 is required"
        usage; exit 1
    fi

    remote="${1}"
    if [[ "${remote}" =~ ^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
        if [[ -z "${passphrase}" ]]; then
            echo "Error: Passphrase is missing for IPv4 connection"
            usage; exit 1
        fi
        ssh_prefix="sshpass -p "${passphrase}""
    fi

    local plug_trust_target="/service_mode"
    if ${ssh_prefix} ssh worldcoin@"${remote}" "! [[ -d ${plug_trust_target} ]]" > /dev/null 2>&1; then
        if [[ -n "${plug_trust}" ]]; then
            ${ssh_prefix} scp "${plug_trust}" worldcoin@"${remote}:/tmp/plug_and_trust.tar.gz"
            ${ssh_prefix} ssh worldcoin@"${remote}" bash --noprofile --norc <<EOF
                set -euo
                tar -xzf /tmp/plug_and_trust.tar.gz -C /tmp/
EOF
            plug_trust_target="/tmp/plug_and_trust"
        else
            echo "Error: --plug-trust option is required"
            usage; exit 1
        fi
    fi

    provision_device "${remote}" "${reprovision}" "${ssh_prefix}" "${plug_trust_target}" "${interactive}"
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    main "$@"
fi
