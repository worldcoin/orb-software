#!/usr/bin/env bash

set -o errexit   # abort on nonzero exit status
set -o errtrace  # pass ERR trap down to functions, substitutions, etc
set -o nounset   # abort on unbound variable
set -o pipefail  # don’t hide errors within pipes

# Function to display usage information
usage() {
    echo "Usage: $0 [OPTIONS] <orb-id> <keypath>

    Options:
    -h, --help                      Display this help message
    -t, --token <bearer>            Bearer token for authentication.
    -b, --backend (stage|prod)      Targets the stage or prod backend.
    -s, --short                     Short upload (skip attestation cert).

    Environment variables (overriden by options):
    FM_CLI_ENV: Must be either 'stage' or 'prod'.
    FM_CLI_ORB_AUTH_INTERNAL_TOKEN: Bearer token for authentication.

    Example:
    $0 -t <token> -b stage 349df8b0 /path/to/provisioning_material"
}

# Function to get Cloudflared access token
get_cloudflared_token() {
    local -r domain="${1}"

    cloudflared access login --quiet "${domain}"
    cloudflared access token -app="${domain}"
}

main() {
    local bearer="${FM_CLI_ORB_AUTH_INTERNAL_TOKEN:-""}"
    local backend="${FM_CLI_ENV:-""}"
    local positional_args=()
    local short=0
    local arg
    while [[ "$#" -gt 0 ]]; do
        arg="${1}"; shift
        case "${arg}" in
            -h|--help)
                usage; exit 0 ;;
            -t|--bearer-token)
                bearer="${1}"; shift ;;
            -b|--backend)
                backend="${1}"; shift ;;
            -s|--short)
                short=1 ;;
            -*)
                echo "Unknown option: ${arg}"
                usage; exit 1 ;;
            *)
                positional_args+=("${arg}") ;;
        esac
    done
    set -- "${positional_args[@]}"

    if [[ $# -ne 2 ]]; then
        echo "must pass <orb-id> <keypath>"
        usage
        exit 1
    fi

    if [[ -z "${bearer}" ]]; then
        echo "Bearer token not found. Please export FM_CLI_ORB_MANAGER_INTERNAL_TOKEN,
        or pass it as an argument: -t <bearer>"
        exit 1
    fi

    if [[ -z "${backend}" ]]; then
        echo "Environment not found. Please export FM_CLI_ENV,
        or pass it as an argument: -b (stage|prod)"
        exit 1
    fi

    if [[ "${backend}" != "prod" && "${backend}" != "stage" ]]; then
        echo "Invalid environment: ${backend}. Must be either 'prod' or 'stage'."
        exit 1
    fi

    local -r orb_id="${1}"
    local -r keypath="${2}"

    # Determine the domain based on the environment
    local domain
    if [[ "${backend}" == "prod" ]]; then
        domain="auth.internal.orb.worldcoin.dev"
    else
        domain="auth.internal.stage.orb.worldcoin.dev"
    fi

    # Ensure the keypath exists
    if [[ ! -d "$keypath" ]]; then
        echo "Error: Keypath directory '$keypath' does not exist."
        exit 1
    fi

    echo "Getting Cloudflared access token..."
    local cf_token
    cf_token="$(get_cloudflared_token "${domain}")"

    # Post attestation certificate
    if [[ ${short} -eq 0 ]]; then
        local certificate
        certificate=$(sed 's/$/\\n/' "${keypath}/f0000013.cert" | tr -d \\n)
        curl --fail --location \
            -H "Authorization: Bearer ${bearer}" \
            -H "cf-access-token: ${cf_token}" \
            -X POST "https://${domain}/api/v1/certificate" \
            -d '{ "orbId": "'"${orb_id}"'", "certificate": "'"${certificate}"'" }'
    fi

    # Post signup key
    local signup_pubkey
    signup_pubkey=$(sed 's/$/\\n/' "${keypath}/sss_70000002_0002_0040.bin" | tr -d \\n)
    curl --fail --location \
        -H "Authorization: Bearer ${bearer}" \
        -H "cf-access-token: ${cf_token}" \
        -X POST "https://${domain}/api/v1/key" \
        -d '{
            "orbId": "'"${orb_id}"'",
            "type": "signup",
            "key": "'"${signup_pubkey}"'",
            "signature": "'$(base64 -w 0 -i "${keypath}/70000002.signature.raw")'",
            "extraData": "'$(base64 -w 0 -i "${keypath}/70000002.extra.raw")'"
        }'

    # Post attestation key
    local attestation_pubkey
    attestation_pubkey=$(sed 's/$/\\n/' "${keypath}/sss_70000001_0002_0040.bin" | tr -d \\n)
    curl --fail --location \
        -H "Authorization: Bearer ${bearer}" \
        -H "cf-access-token: ${cf_token}" \
        -X POST "https://${domain}/api/v1/key" \
        -d '{
            "orbId": "'"${orb_id}"'",
            "type": "attestation",
            "key": "'"${attestation_pubkey}"'",
            "signature": "'$(base64 -w 0 -i "${keypath}/70000001.signature.raw")'",
            "extraData": "'$(base64 -w 0 -i "${keypath}/70000001.extra.raw")'"
        }'

    # Post chip ID
    curl --fail --location \
        -H "Authorization: Bearer ${bearer}" \
        -H "cf-access-token: ${cf_token}" \
        -X POST "https://${domain}/api/v1/key" \
        -d '{
            "orbId": "'"${orb_id}"'",
            "type": "chipid",
            "key": "'"$(base64 -w 0 -i "${keypath}/7fff0206.chip_id.raw")"'",
            "signature": "'$(base64 -w 0 -i "${keypath}/7fff0206.signature.raw")'",
            "extraData": "'$(base64 -w 0 -i "${keypath}/7fff0206.extra.raw")'"
        }'
}

# Ensure that main only runs when called as a script
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    main "$@"
fi

