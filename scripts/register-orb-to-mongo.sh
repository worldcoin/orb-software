#!/usr/bin/env bash

set -o errexit
set -o errtrace
set -o nounset
set -o pipefail

CORE_APP_REGISTRATION_URL="https://api.operator.worldcoin.org/v1/graphql"

usage() {
    cat >&2 <<EOF
Usage: $0 [options] <orb-ids-file>

Options:
    -h, --help                          Display this help message
    -t, --token <bearer_token>          Authorization bearer token
    -b, --backend (stage|prod)          Targets the stage or prod backend
    -r, --release (dev|prod)            Release type
    -v, --hardware-version <version>    Hardware version

Example:
    $0 -r dev -v EVT1 orb_ids.txt

Description:
    Registers Orb IDs from a provided file with Mongo and Core-App.
EOF
}

get_cloudflared_token() {
    local domain="$1"

    cloudflared access login --quiet "${domain}"
    cloudflared access token --app="${domain}"
}

register_orb() {
    local orb_id="$1"
    local cf_token="$2"
    local core_bearer="$3"
    local mongo_bearer="$4"
    local release_type="$5"
    local hardware_version="$6"
    local domain="$7"

    local orb_name_file="${orb_id}/orb-name"
    local orb_id_base="$(basename "${orb_id}")"

    local is_dev
    case "${release_type}" in
        "dev") is_dev="true";;
        "prod") is_dev="false";;
        *) echo "Error: Invalid release type specified." >&2; exit 1;;
    esac

    mkdir -p "$(dirname "${orb_name_file}")"
    echo "${orb_id_base}"
    curl --fail --location \
        --request POST "${domain}/api/v1/orbs/${orb_id_base}" \
        --header 'Content-Type: application/json' \
        --header "Authorization: Bearer ${mongo_bearer}" \
        --header "cf-access-token: ${cf_token}" \
        --data '{"BuildVersion": "'"${hardware_version}"'", "ManufacturerName": "TFH_Jabil"}' \
        | jq -re '.name' > "${orb_name_file}"

    local orb_name
    orb_name="$(cat "${orb_name_file}")"

    # !! DO NOT CHANGE !!
    curl --fail --location --request POST "${CORE_APP_REGISTRATION_URL}" \
        --header "Authorization: Bearer ${core_bearer}" \
        --header 'Content-Type: application/json' \
        --data-raw '{
            "query":"mutation InsertOrb($deviceId: String, $name: String!) { insert_orb(objects: [{name: $name, deviceId: $deviceId, status: FLASHED, deviceType: '"${hardware_version}"', isDevelopment: '"${is_dev}"'}], on_conflict: {constraint: orb_pkey}) {affected_rows}}",
            "variables": {"deviceId": "'"${orb_id_base}"'", "name": "'"${orb_name}"'"}
        }' | jq -re 'if .data.insert_orb.affected_rows == 1 then true else error("Failed to register Orb") end'

    echo "Orb ${orb_id_base} registered successfully."
}

main() {
    local mongo_bearer="${FM_CLI_ORB_MANAGER_INTERNAL_TOKEN:-}"
    local core_bearer="${HARDWARE_TOKEN_PRODUCTION:-}"
    local hardware_version="${HARDWARE_VERSION:-}"
    local backend="${FM_CLI_ENV:-}"
    local release_type=""

    while [[ $# -gt 0 ]]; do
        case "$1" in
            -h|--help) usage; exit 0;;
            -b|--backend) backend="$2"; shift;;
            -r|--release) release_type="$2"; shift;;
            -v|--hardware-version) hardware_version="$2"; shift;;
            -t|--token) core_bearer="$2"; shift;;
            -*) echo "Unknown option: $1" >&2; usage; exit 1;;
            *) break;;
        esac
        shift
    done

    if [[ $# -ne 1 ]]; then
        usage; exit 1
    fi

    local orb_ids_file="$1"

    case "${backend}" in
        "stage") domain="https://management.internal.stage.orb.worldcoin.dev";;
        "prod") domain="https://management.internal.orb.worldcoin.dev";;
        *) echo "Error: Invalid backend specified." >&2; exit 1;;
    esac

    cf_token="$(get_cloudflared_token "${domain}")"

    while IFS= read -r orb_id || [[ -n "$orb_id" ]]; do
        orb_id_path="$(realpath "$orb_id")"
        if [[ ! -d "$orb_id_path" ]]; then
          mkdir -p "${orb_id_path}"
        fi
        register_orb "$orb_id_path" "$cf_token" "$core_bearer" "$mongo_bearer" "$release_type" "$hardware_version" "$domain"
    done < "$orb_ids_file"
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    main "$@"
fi