#!/usr/bin/env bash

set -o errexit
set -o errtrace
set -o nounset
set -o pipefail

if [[ -z "${NO_COLOR:-}" ]]; then
    RED='\033[0;31m'
    YELLOW='\033[0;33m'
    GREEN='\033[0;32m'
    CYAN='\033[0;36m'
    BOLD='\033[1m'
    NC='\033[0m'
else
    RED=''
    YELLOW=''
    GREEN=''
    CYAN=''
    BOLD=''
    NC=''
fi

log_info() {
    echo -e "${GREEN}[INFO]${NC} $*" >&2
}
log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $*" >&2
}
log_error() {
    echo -e "${RED}[ERROR]${NC} $*" >&2
}
log_step() {
    echo -e "${CYAN}==>${NC} $*" >&2
}

for cmd in bc dd tune2fs setfacl e2fsck mount umount ssh-keygen jq curl cloudflared; do
    if ! command -v "$cmd" &>/dev/null; then
        log_error "Command '$cmd' is required but not found on PATH."
        exit 1
    fi
done

SCRIPT_DIR="$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )"
BUILD_DIR="${SCRIPT_DIR}/build"
ARTIFACTS_DIR="${SCRIPT_DIR}/artifacts"
CORE_APP_REGISTRATION_URL="https://api.operator.worldcoin.org/v1/graphql"
PERSISTENT_JOURNALED_IMG="${BUILD_DIR}/persistent-journaled.img"
PERSISTENT_JOURNALED_SIZE="$(echo "10*1024^2" | bc)"
PERSISTENT_IMG="${BUILD_DIR}/persistent.img"
PERSISTENT_SIZE="$(echo "1024^2" | bc)"

cleanup() {
    local exit_code=$?
    if [[ -n "${mount_point:-}" ]]; then
        if mountpoint -q "${mount_point}"; then
            log_warn "Attempting to unmount ${mount_point}"
            umount "${mount_point}" || log_error "Failed to unmount ${mount_point}"
        fi
        # Attempt to remove the dir; ignore errors
        rmdir "${mount_point}" 2>/dev/null || true
    fi
    exit $exit_code
}
trap cleanup EXIT

usage() {
    cat >&2 <<EOF
Usage: $0 [options] <number>

Options:
    -h, --help                          Display this help message
    -t, --token <bearer_token>          Authorization bearer token
    -b, --backend (stage|prod)          Targets the stage or prod backend
    -r, --release (dev|prod)            Release type
    -v, --hardware-version <version>    Hardware version
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
    Generates a set number of unique Orb IDs, the corresponding persistent
    image, and registers them with Core-App.
EOF
}

##
# Obtains a Cloudflare access token for the specified domain.
# - Logs to stderr
# - Echoes token to stdout so it can be captured in a variable.
##
get_cloudflared_token() {
    local domain="$1"

    log_step "Logging in to Cloudflare Access for domain: ${domain}"
    cloudflared access login --quiet "${domain}"

    log_info "Fetching Cloudflare access token"
    # ONLY the final echo of the token goes to stdout:
    cloudflared access token --app="${domain}"
}

##
# Generates an Orb:  creates the orb record, sets the channel, obtains the token,
# and copies persistent images.
# - Logs to stderr
# - Echoes the orb_id to stdout as the return value at the end
##
generate_orb() {
    local bearer="$1"
    local domain="$2"
    local hardware_version="$3"
    local cf_token="$4"
    local channel="$5"
    local mount_target="$6"

    log_info "Generating new SSH keypair to derive Orb ID..."
    ssh-keygen -N '' -o -a 100 -t ed25519 -q -f "${BUILD_DIR}/uid"

    # Derive Orb ID from public key
    local orb_id
    orb_id="$(cut -d' ' -f2 < "${BUILD_DIR}/uid.pub" \
        | tr -d '\n' \
        | sha256sum \
        | cut -c1-8)"

    local jet_artifacts_dir="${ARTIFACTS_DIR}/${orb_id}"
    local orb_name_file="${jet_artifacts_dir}/orb-name"
    local orb_token_file="${jet_artifacts_dir}/token"

    mkdir -p "${jet_artifacts_dir}"
    mv "${BUILD_DIR}/uid"* "${jet_artifacts_dir}/"

    log_info "Creating Orb record in Management API for Orb ID: ${orb_id}"
    curl --fail --location \
        --request POST "${domain}/api/v1/orbs/${orb_id}" \
        --header 'Content-Type: application/json' \
        --header "Authorization: Bearer ${bearer}" \
        --header "cf-access-token: ${cf_token}" \
        --data '{
            "BuildVersion": "'"${hardware_version}"'",
            "ManufacturerName": "TFH_Jabil"
        }' \
        | jq -re '.name' \
        > "${orb_name_file}"

    if [[ ! -r "${orb_name_file}" || ! -s "${orb_name_file}" ]]; then
        log_error "Orb Name was empty! Cleaning up..."
        rm -rf "${jet_artifacts_dir}"
        exit 1
    fi

    log_info "Setting Orb channel to '${channel}'"
    curl --fail --location \
        --request POST "${domain}/api/v1/orbs/${orb_id}/channel" \
        --header 'Content-Type: application/json' \
        --header "Authorization: Bearer ${bearer}" \
        --header "cf-access-token: ${cf_token}" \
        --data '{
            "channel": "'"${channel}"'"
        }'

    log_info "Fetching Orb token from Management API"
    curl --fail --location \
        --request POST "${domain}/api/v1/tokens?orbId=${orb_id}" \
        --header 'Content-Type: application/json' \
        --header "Authorization: Bearer ${bearer}" \
        --header "cf-access-token: ${cf_token}" \
        --data-raw '{}' \
        | jq -re 'if (.token | type) == "string" then .token else error("expected a string!") end' \
        > "${orb_token_file}"

    if [[ ! -r "${orb_token_file}" || ! -s "${orb_token_file}" ]]; then
        log_error "Orb token was invalid! Cleaning up..."
        rm -rf "${jet_artifacts_dir}"
        exit 1
    fi

    log_info "Copying base persistent images into artifacts directory for ${orb_id}"
    cp "${PERSISTENT_IMG}" "${jet_artifacts_dir}/persistent.img"
    cp "${PERSISTENT_JOURNALED_IMG}" "${jet_artifacts_dir}/persistent-journaled.img"

    log_info "Mounting persistent.img for Orb ID: ${orb_id}"
    mount "${jet_artifacts_dir}/persistent.img" "${mount_target}"
    install -o 0 -g 0 -m 644 "${orb_name_file}"  "${mount_target}/orb-name"
    install -o 0 -g 0 -m 644 "${orb_token_file}" "${mount_target}/token"
    sync
    umount "${mount_target}"

    log_info "Mounting persistent-journaled.img for Orb ID: ${orb_id}"
    mount "${jet_artifacts_dir}/persistent-journaled.img" "${mount_target}"
    install -o 0 -g 0 -m 644 "${orb_name_file}"  "${mount_target}/orb-name"
    install -o 0 -g 0 -m 644 "${orb_token_file}" "${mount_target}/token"
    sync
    umount "${mount_target}"

    echo "${orb_id}"
}

##
# Creates a base persistent.img and persistent-journaled.img with necessary JSON files.
# - Logs only (no “return” value) => no stdout except commands that generate no text
##
create_base_persistent_image() {
    local mount_target="$1"
    log_step "Creating base persistent and persistent-journaled images..."

    log_info "Creating empty images of size ${PERSISTENT_SIZE} and ${PERSISTENT_JOURNALED_SIZE}"
    dd if=/dev/zero of="${PERSISTENT_IMG}" bs=4096 count="$(echo "${PERSISTENT_SIZE} / 4096" | bc)" status=none
    dd if=/dev/zero of="${PERSISTENT_JOURNALED_IMG}" bs=4096 count="$(echo "${PERSISTENT_JOURNALED_SIZE} / 4096" | bc)" status=none

    log_info "Formatting persistent-journaled.img with ext4 (with journal)"
    mke2fs -q -t ext4 -E root_owner=0:1000 "${PERSISTENT_JOURNALED_IMG}"

    log_info "Formatting persistent.img with ext4 (no journal)"
    mke2fs -q -t ext4 -O ^has_journal -E root_owner=0:1000 "${PERSISTENT_IMG}"

    log_info "Setting ACL support on both images"
    tune2fs -o acl "${PERSISTENT_JOURNALED_IMG}" >/dev/null
    tune2fs -o acl "${PERSISTENT_IMG}"          >/dev/null

    log_info "Mounting persistent.img and installing baseline JSON files"
    mount -o loop "${PERSISTENT_IMG}" "${mount_target}"
    install -o 0    -g 1000 -m 664 "${BUILD_DIR}/components.json"   "${mount_target}/components.json"
    install -o 1000 -g 1000 -m 664 "${BUILD_DIR}/calibration.json"  "${mount_target}/calibration.json"
    install -o 1000 -g 1000 -m 664 "${BUILD_DIR}/versions.json"     "${mount_target}/versions.json"
    setfacl -d -m u::rwx,g::rwx,o::rx "${mount_target}"
    sync
    umount "${mount_target}"

    log_info "Mounting persistent-journaled.img and installing baseline JSON files"
    mount -o loop "${PERSISTENT_JOURNALED_IMG}" "${mount_target}"
    install -o 0    -g 1000 -m 664 "${BUILD_DIR}/components.json"  "${mount_target}/components.json"
    install -o 1000 -g 1000 -m 664 "${BUILD_DIR}/calibration.json" "${mount_target}/calibration.json"
    install -o 1000 -g 1000 -m 664 "${BUILD_DIR}/versions.json"    "${mount_target}/versions.json"
    setfacl -d -m u::rwx,g::rwx,o::rx "${mount_target}"
    sync
    umount "${mount_target}"

    log_info "Base persistent images created successfully."
}

##
# Registers an orb with the Core-App service.
# - Logs only (no “return” value).
##
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

    log_info "Registering Orb ID=${orb_id} with Core-App"
    curl --fail --location --request POST "${CORE_APP_REGISTRATION_URL}" \
        --header "Authorization: Bearer ${bearer}" \
        --header 'Content-Type: application/json' \
        --data-raw '{
            "query": "mutation InsertOrb($deviceId: String, $name: String!) {
                insert_orb(
                    objects: [{
                        name: $name,
                        deviceId: $deviceId,
                        status: FLASHED,
                        deviceType: '"${hardware_version}"',
                        isDevelopment: '"${is_dev}"'
                    }],
                    on_conflict: {constraint: orb_pkey}
                ) {
                    affected_rows
                }
            }",
            "variables": {"deviceId": "'"${orb_id}"'", "name": "'"${orb_name}"'"}
        }' \
        | jq -re 'if .data.insert_orb.affected_rows == 1 then true else error("Failed to register Orb") end'

    log_info "Orb ${orb_id} registered successfully."
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

    # Parse CLI arguments
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

    if [[ -z "${release_type}" ]]; then
        log_error "Must provide --release <RELEASE> argument. See --help."
        exit 1
    fi

    if [[ ${#positional_args[@]} -ne 1 ]]; then
        log_error "Error: <number> of Orb IDs to generate is required."
        usage
        exit 1
    fi
    num_ids="${positional_args[0]}"

    # Confirm required items
    if [[ -z "${bearer}" || -z "${hardware_token}" || -z "${hardware_version}" || -z "${backend}" ]]; then
        log_error "Missing required arguments."
        echo "Bearer:            ${bearer:-N/A}"            >&2
        echo "Hardware Token:    ${hardware_token:-N/A}"    >&2
        echo "Hardware Version:  ${hardware_version:-N/A}"  >&2
        echo "Backend:           ${backend:-N/A}"           >&2
        usage
        exit 1
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

    log_step "Obtaining Cloudflared access token..."
    cf_token="$(get_cloudflared_token "${domain}")"
    log_info "Cloudflared token obtained successfully."

    mount_point="${BUILD_DIR}/.loop"
    install -o 1000 -g 1000 -m 755 -d "${mount_point}"

    create_base_persistent_image "${mount_point}"

    for i in $(seq 1 "${num_ids}"); do
        log_step "Generating Orb ID #${i} of ${num_ids}..."
        orb_id="$(generate_orb "${bearer}" "${domain}" "${hardware_version}" "${cf_token}" "${channel}" "${mount_point}")"
        register_orb "${orb_id}" "${hardware_token}" "${release_type}" "${hardware_version}"
        log_info "Successfully processed Orb: ${orb_id}"
        echo >&2
    done

    log_step "All ${num_ids} Orb IDs generated and registered successfully."
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    main "$@"
fi
