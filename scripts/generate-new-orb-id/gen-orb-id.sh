#!/usr/bin/env bash

set -o errexit
set -o errtrace
set -o nounset
set -o pipefail

SCRIPT_DIR=$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )
BUILD_DIR="${SCRIPT_DIR}/build"
ARTIFACTS_DIR="${SCRIPT_DIR}/artifacts"
CORE_APP_REGISTRATION_URL="https://api.operator.worldcoin.org/v1/graphql"
PERSISTENT_JOURNALED_IMG="${BUILD_DIR}/persistent-journaled.img"
PERSISTENT_JOURNALED_SIZE="$(echo "10*1024^2" | bc)"
PERSISTENT_IMG="${BUILD_DIR}/persistent.img"
PERSISTENT_SIZE="$(echo "1024^2" | bc)"

# Display usage information
usage() {
    echo "Usage: $0 [options] <number>"
    echo "
Options:
    -h, --help                          Display this help message
    -t, --token <bearer_token>          Authorization bearer token
    -b, --backend (stage|prod)          Targets the stage or prod backend
    -r, --release (dev|prod)            Release type
    -v, --hardware-revision <version>   Hardware version
    -c, --registration-token <token>    Registration token for Core-App

Environment Variables (overridden by options):
    FM_CLI_ENV: stage or prod
    FM_CLI_ORB_MANAGER_INTERNAL_TOKEN: Provisioner token
    HARDWARE_TOKEN_PRODUCTION: Core-App provisioner token
    HARDWARE_VERSION: Hardware version

Required Files in ${BUILD_DIR}:
    components.json
    calibration.json
    versions.json

Example:
    $0 -r dev -v EVT1 10

Description:
    Generates a set number of unique Orb IDs, the corresponding persistent image, and registers them with Core-App."
}

get_cloudflared_token() {
    local domain="$1"
    cloudflared access login --quiet "${domain}"
    cloudflared access token --app="${domain}"
}

generate_orb() {
    local bearer="$1"
    local domain="$2"
    local hardware_version="$3"
    local cf_token="$4"
    local channel="$5"
    local mount_target="$6"

    # Generate a unique orb ID
    ssh-keygen -N '' -o -a 100 -t ed25519 -q -f "${BUILD_DIR}/uid"
    local orb_id
    orb_id=$(cut -d' ' -f2 < "${BUILD_DIR}/uid.pub" | tr -d '\n' | sha256sum | cut -c1-8)

    local jet_artifacts_dir="${ARTIFACTS_DIR}/${orb_id}"
    local orb_name_file="${jet_artifacts_dir}/orb-name"
    local orb_token_file="${jet_artifacts_dir}/token"
    local hardware_version_file="${jet_artifacts_dir}/hardware_version"
    mkdir -p "${jet_artifacts_dir}"
    echo "${hardware_version}" > "${hardware_version_file}"
    mv "${BUILD_DIR}/uid"* "${jet_artifacts_dir}/"

    curl --fail --location \
        --request POST "${domain}/api/v1/orbs/${orb_id}" \
        --header 'Content-Type: application/json' \
        --header "Authorization: Bearer ${bearer}" \
        -H "cf-access-token: ${cf_token}" \
        --data '{
            "BuildVersion": "'"${hardware_version}"'",
            "ManufacturerName": "TFH_Jabil"
        }' | jq -re '.name' > "${orb_name_file}"

    if [[ ! -r "${orb_name_file}" || ! -s "${orb_name_file}" ]]; then
        echo "Orb Name was empty!"
        rm -rf "${jet_artifacts_dir}"
        exit 1
    fi

    curl --fail --location \
        --request POST "${domain}/api/v1/orbs/${orb_id}/channel" \
        --header 'Content-Type: application/json' \
        --header "Authorization: Bearer ${bearer}" \
        -H "cf-access-token: ${cf_token}" \
        --data '{
            "channel": "'"${channel}"'"
        }'

    curl --fail --location \
        --request POST "${domain}/api/v1/tokens?orbId=${orb_id}" \
        --header 'Content-Type: application/json' \
        --header "Authorization: Bearer ${bearer}" \
        -H "cf-access-token: ${cf_token}" \
        --data-raw '{}' \
        | jq -re 'if (.token | type) == "string" then .token else error("expected a string!") end' \
        > "${orb_token_file}"

    # Ensure ${orb_token_file} is readable and not empty
    if ! [[ -r "${orb_token_file}" && -s "${orb_token_file}" ]]; then
        echo "Token was invalid!" >&2
        rm -rf "${jet_artifacts_dir}"
        exit 1
    fi

    cp "${PERSISTENT_IMG}" "${jet_artifacts_dir}/persistent.img"
    cp "${PERSISTENT_JOURNALED_IMG}" "${jet_artifacts_dir}/persistent-journaled.img"

    # Copy necessary files to persistent.img
    mount "${jet_artifacts_dir}/persistent.img" "${mount_target}"
    install -o 0 -g 0 -m 644 "${orb_name_file}" "${mount_target}/orb-name"
    install -o 0 -g 0 -m 644 "${orb_token_file}" "${mount_target}/token"
    sync
    umount "${mount_target}"

    # Copy necessary files to persistent-journaled.img
    mount "${jet_artifacts_dir}/persistent-journaled.img" "${mount_target}"
    install -o 0 -g 0 -m 644 "${orb_name_file}" "${mount_target}/orb-name"
    install -o 0 -g 0 -m 644 "${orb_token_file}" "${mount_target}/token"
    sync
    umount "${mount_target}"

    echo "${orb_id}"
}

create_base_persistent_image() {
    local mount_target="$1"
    local hardware_version="$2"

    dd if=/dev/zero of="${PERSISTENT_IMG}" bs=4096 count="$(echo "${PERSISTENT_SIZE} / 4096" | bc)" status=none
    dd if=/dev/zero of="${PERSISTENT_JOURNALED_IMG}" bs=4096 count="$(echo "${PERSISTENT_JOURNALED_SIZE} / 4096" | bc)" status=none
    mke2fs -q -t ext4 -E root_owner=0:1000 "${PERSISTENT_JOURNALED_IMG}"
    mke2fs -q -t ext4 -O ^has_journal -E root_owner=0:1000 "${PERSISTENT_IMG}"
    tune2fs -o acl "${PERSISTENT_JOURNALED_IMG}" > /dev/null
    tune2fs -o acl "${PERSISTENT_IMG}" > /dev/null

    # Copy necessary files to persistent.img
    mount "${PERSISTENT_IMG}" "${mount_target}"
    install -o 0 -g 1000 -m 664 "${BUILD_DIR}/components.json" "${mount_target}/components.json"
    install -o 1000 -g 1000 -m 664 "${BUILD_DIR}/calibration.json" "${mount_target}/calibration.json"
    install -o 1000 -g 1000 -m 664 "${BUILD_DIR}/versions.json" "${mount_target}/versions.json"
    install -o 1000 -g 1000 -m 664 /dev/null "${mount_target}/hardware_version"
    echo "${hardware_version}" > "${mount_target}/hardware_version"
    setfacl -d -m u::rwx,g::rwx,o::rx "${mount_target}"
    sync
    umount "${mount_target}"

    # Copy necessary files to persistent-journaled.img
    mount "${PERSISTENT_JOURNALED_IMG}" "${mount_target}"
    install -o 0 -g 1000 -m 664 "${BUILD_DIR}/components.json" "${mount_target}/components.json"
    install -o 1000 -g 1000 -m 664 "${BUILD_DIR}/calibration.json" "${mount_target}/calibration.json"
    install -o 1000 -g 1000 -m 664 "${BUILD_DIR}/versions.json" "${mount_target}/versions.json"
    install -o 1000 -g 1000 -m 664 /dev/null "${mount_target}/hardware_version"
    echo "${hardware_version}" > "${mount_target}/hardware_version"
    setfacl -d -m u::rwx,g::rwx,o::rx "${mount_target}"
    sync
    umount "${mount_target}"
}

register_orb() {
    local orb_id="$1"
    local bearer="$2"
    local release_type="$3"
    local hardware_version="$4"

    local orb_name
    orb_name="$(cat "${ARTIFACTS_DIR}/${orb_id}/orb-name")"

    local is_dev
    case "${release_type}" in
        "dev")
            is_dev="true" ;;
        "prod")
            is_dev="false" ;;
        *)
            echo "Error: Invalid release type specified." >&2
            usage; exit 1 ;;
    esac

    curl --fail --location --request POST "${CORE_APP_REGISTRATION_URL}" \
        --header "Authorization: Bearer ${bearer}" \
        --header 'Content-Type: application/json' \
        --data-raw '{
            "query":"mutation InsertOrb($deviceId: String, $name: String!) { insert_orb(objects: [{name: $name, deviceId: $deviceId, status: FLASHED, deviceType: '"${hardware_version}"', isDevelopment: '"${is_dev}"'}], on_conflict: {constraint: orb_pkey}) {affected_rows}}",
            "variables": {"deviceId": "'"${orb_id}"'", "name": "'"${orb_name}"'"}
        }' | jq -re 'if .data.insert_orb.affected_rows == 1 then true else error("Failed to register Orb") end'
}

main() {
    local bearer="${FM_CLI_ORB_MANAGER_INTERNAL_TOKEN:-}"
    local hardware_token="${HARDWARE_TOKEN_PRODUCTION:-}"
    local hardware_version="${HARDWARE_VERSION:-}"
    local backend="${FM_CLI_ENV:-}"
    local release_type

    local arg
    local num_ids
    local positional_args=()
    while [[ $# -gt 0 ]]; do
        arg="$1" ; shift
        case "${arg}" in
            -h|--help)
                usage; exit 0 ;;
            -t|--token)
                bearer="${1}"; shift ;;
            -b|--backend)
                backend="${1}"; shift ;;
            -r|--release)
                release_type="${1}"; shift ;;
            -v|--hardware-version)
                hardware_version="${1}"; shift ;;
            -c|--registration-token)
                hardware_token="${1}"; shift ;;
            -*)
                echo "Unknown option: ${1}" >&2
                usage; exit 1 ;;
            *)
                positional_args+=("${arg}") ;;
        esac
    done

    if [[ -z "${release_type+x}" ]]; then
        echo "must provide --release <RELEASE> arg. see --help" >&2
        exit 1
    fi

    if [[ ${#positional_args[@]} -ne 1 ]]; then
        echo "Error: <id_num> is required." >&2
        usage; exit 1
    fi
    num_ids="${positional_args[0]}"

    if [[ -z "${bearer}" || -z "${hardware_token}" || -z "${hardware_version}" || -z "${backend}" ]]; then
        echo "Error: Missing required arguments."
        echo "Bearer: ${bearer}"
        echo "Hardware Token: ${hardware_token}"
        echo "Hardware Version: ${hardware_version}"
        echo "Backend: ${backend}"
        usage; exit 1
    fi

    local domain
    local cf_token
    local channel
    case "${backend}" in
        "stage")
            domain="https://management.internal.stage.orb.worldcoin.dev"
            channel="internal-testing" ;;
        "prod")
            domain="https://management.internal.orb.worldcoin.dev"
            channel="jabil-evt5" ;;
        *)
            echo "Error: Invalid backend specified." >&2
            usage; exit 1 ;;
    esac

    echo "Getting Cloudflared access token..."
    cf_token="$(get_cloudflared_token "${domain}")"

    mount_point="${BUILD_DIR}/.loop"
    install -o 1000 -g 1000 -m 755 -d "${mount_point}"
    create_base_persistent_image "${mount_point}" "${hardware_version}"

    local orb_id
    for i in $(seq 1 "${num_ids}"); do
        echo "Generating Orb ID #${i}..."
        orb_id="$(generate_orb "${bearer}" "${domain}" "${hardware_version}" "${cf_token}" "${channel}" "${mount_point}")"
        register_orb "${orb_id}" "${hardware_token}" "${release_type}" "${hardware_version}"
    done
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    main "$@"
fi

