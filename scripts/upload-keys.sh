#!/usr/bin/env bash

set -o errexit
set -o errtrace
set -o nounset
set -o pipefail

usage() {
    cat <<'EOF'
Usage: script.sh [OPTIONS] <orb-id> <keypath>

Options:
  -h, --help                      Display this help message
  -t, --bearer-token <bearer>    Bearer token for authentication
  -b, --backend (stage|prod)     Target the stage or prod backend
  -s, --short                    Short upload (skip attestation cert)
  -n, --dry-run                  Print/write payloads without making curl requests

Environment variables (overridden by options):
  FM_CLI_ENV: Must be either 'stage' or 'prod'
  FM_CLI_ORB_AUTH_INTERNAL_TOKEN: Bearer token for authentication

Example:
  script.sh -t <token> -b stage 349df8b0 /path/to/provisioning_material
EOF
}

get_cloudflared_token() {
    local -r domain="${1}"
    cloudflared access login --quiet "${domain}"
    cloudflared access token -app="${domain}"
}

require_file() {
    local -r path="${1}"
    if [[ ! -f "${path}" ]]; then
        echo "Error: Required file not found: ${path}" >&2
        exit 1
    fi
}

make_key_payload() {
    local -r orb_id="${1}"
    local -r key_type="${2}"
    local -r key_file="${3}"
    local -r sig_file="${4}"
    local -r extra_file="${5}"

    require_file "${key_file}"
    require_file "${sig_file}"
    require_file "${extra_file}"

    local key_value
    local signature_b64
    local extra_b64

    key_value="$(cat "${key_file}")"
    signature_b64="$(base64 -w 0 "${sig_file}")"
    extra_b64="$(base64 -w 0 "${extra_file}")"

    jq -n \
        --arg orbId "${orb_id}" \
        --arg type "${key_type}" \
        --arg key "${key_value}" \
        --arg signature "${signature_b64}" \
        --arg extraData "${extra_b64}" \
        '{
            orbId: $orbId,
            type: $type,
            key: $key,
            signature: $signature,
            extraData: $extraData,
            active: true
        }'
}

make_chipid_payload() {
    local -r orb_id="${1}"
    local -r chipid_file="${2}"
    local -r sig_file="${3}"
    local -r extra_file="${4}"

    require_file "${chipid_file}"
    require_file "${sig_file}"
    require_file "${extra_file}"

    local chipid_b64
    local signature_b64
    local extra_b64

    chipid_b64="$(base64 -w 0 "${chipid_file}")"
    signature_b64="$(base64 -w 0 "${sig_file}")"
    extra_b64="$(base64 -w 0 "${extra_file}")"

    jq -n \
        --arg orbId "${orb_id}" \
        --arg key "${chipid_b64}" \
        --arg signature "${signature_b64}" \
        --arg extraData "${extra_b64}" \
        '{
            orbId: $orbId,
            type: "chipid",
            key: $key,
            signature: $signature,
            extraData: $extraData,
            active: true
        }'
}

make_certificate_payload() {
    local -r orb_id="${1}"
    local -r cert_file="${2}"

    require_file "${cert_file}"

    local certificate_value
    certificate_value="$(cat "${cert_file}")"

    jq -n \
        --arg orbId "${orb_id}" \
        --arg certificate "${certificate_value}" \
        '{
            orbId: $orbId,
            certificate: $certificate
        }'
}

make_dry_run_json() {
    local -r orb_id="${1}"
    local -r keypath="${2}"

    local attestation_key_file="${keypath}/sss_70000001_0002_0040.bin"
    local signup_key_file="${keypath}/sss_70000002_0002_0040.bin"
    local chipid_key_file="${keypath}/7fff0206.chip_id.raw"

    local attestation_sig_file="${keypath}/70000001.signature.raw"
    local attestation_extra_file="${keypath}/70000001.extra.raw"
    local signup_sig_file="${keypath}/70000002.signature.raw"
    local signup_extra_file="${keypath}/70000002.extra.raw"
    local chipid_sig_file="${keypath}/7fff0206.signature.raw"
    local chipid_extra_file="${keypath}/7fff0206.extra.raw"

    require_file "${attestation_key_file}"
    require_file "${signup_key_file}"
    require_file "${chipid_key_file}"
    require_file "${attestation_sig_file}"
    require_file "${attestation_extra_file}"
    require_file "${signup_sig_file}"
    require_file "${signup_extra_file}"
    require_file "${chipid_sig_file}"
    require_file "${chipid_extra_file}"

    local attestation_key signup_key chipid_key
    local attestation_sig attestation_extra signup_sig signup_extra chipid_sig chipid_extra

    attestation_key="$(cat "${attestation_key_file}")"
    signup_key="$(cat "${signup_key_file}")"
    chipid_key="$(base64 -w 0 "${chipid_key_file}")"

    attestation_sig="$(base64 -w 0 "${attestation_sig_file}")"
    attestation_extra="$(base64 -w 0 "${attestation_extra_file}")"
    signup_sig="$(base64 -w 0 "${signup_sig_file}")"
    signup_extra="$(base64 -w 0 "${signup_extra_file}")"
    chipid_sig="$(base64 -w 0 "${chipid_sig_file}")"
    chipid_extra="$(base64 -w 0 "${chipid_extra_file}")"

    jq -n \
        --arg orbId "${orb_id}" \
        --arg attestationKey "${attestation_key}" \
        --arg attestationSig "${attestation_sig}" \
        --arg attestationExtra "${attestation_extra}" \
        --arg chipidKey "${chipid_key}" \
        --arg chipidSig "${chipid_sig}" \
        --arg chipidExtra "${chipid_extra}" \
        --arg signupKey "${signup_key}" \
        --arg signupSig "${signup_sig}" \
        --arg signupExtra "${signup_extra}" \
        '[
            {
                orbId: $orbId,
                key: $attestationKey,
                type: "attestation",
                active: true,
                extraData: { "$binary": { "base64": $attestationExtra, "subType": "00" } },
                signature: { "$binary": { "base64": $attestationSig, "subType": "00" } }
            },
            {
                orbId: $orbId,
                type: "chipid",
                active: true,
                key: $chipidKey,
                extraData: { "$binary": { "base64": $chipidExtra, "subType": "00" } },
                signature: { "$binary": { "base64": $chipidSig, "subType": "00" } }
            },
            {
                orbId: $orbId,
                key: $signupKey,
                type: "signup",
                active: true,
                extraData: { "$binary": { "base64": $signupExtra, "subType": "00" } },
                signature: { "$binary": { "base64": $signupSig, "subType": "00" } }
            }
        ]'
}

post_json() {
    local -r url="${1}"
    local -r bearer="${2}"
    local -r cf_token="${3}"
    local -r payload="${4}"

    curl --fail --location \
        -H "Authorization: Bearer ${bearer}" \
        -H "cf-access-token: ${cf_token}" \
        -H "Content-Type: application/json" \
        -X POST "${url}" \
        -d "${payload}"
}

main() {
    local bearer="${FM_CLI_ORB_AUTH_INTERNAL_TOKEN:-}"
    local backend="${FM_CLI_ENV:-}"
    local positional_args=()
    local short=0
    local dry_run=0
    local arg

    while [[ "$#" -gt 0 ]]; do
        arg="${1}"
        shift
        case "${arg}" in
            -h|--help)
                usage
                exit 0
                ;;
            -t|--bearer-token|--token)
                bearer="${1}"
                shift
                ;;
            -b|--backend)
                backend="${1}"
                shift
                ;;
            -s|--short)
                short=1
                ;;
            -n|--dry-run)
                dry_run=1
                ;;
            -*)
                echo "Unknown option: ${arg}" >&2
                usage
                exit 1
                ;;
            *)
                positional_args+=("${arg}")
                ;;
        esac
    done

    set -- "${positional_args[@]}"

    if [[ $# -ne 2 ]]; then
        echo "Error: must pass <orb-id> <keypath>" >&2
        usage
        exit 1
    fi

    local -r orb_id="${1}"
    local -r keypath="${2}"

    if [[ ! -d "${keypath}" ]]; then
        echo "Error: Keypath directory '${keypath}' does not exist." >&2
        exit 1
    fi

    if [[ ${dry_run} -eq 0 ]]; then
        if [[ -z "${bearer}" ]]; then
            echo "Bearer token not found. Please export FM_CLI_ORB_AUTH_INTERNAL_TOKEN, or pass it as an argument: -t <bearer>" >&2
            exit 1
        fi

        if [[ -z "${backend}" ]]; then
            echo "Environment not found. Please export FM_CLI_ENV, or pass it as an argument: -b (stage|prod)" >&2
            exit 1
        fi

        if [[ "${backend}" != "prod" && "${backend}" != "stage" ]]; then
            echo "Invalid environment: ${backend}. Must be either 'prod' or 'stage'." >&2
            exit 1
        fi
    fi

    local domain
    if [[ "${backend}" == "prod" ]]; then
        domain="auth.internal.orb.worldcoin.dev"
    else
        domain="auth.internal.stage.orb.worldcoin.dev"
    fi

    if [[ ${dry_run} -eq 1 ]]; then
        echo "=== DRY RUN MODE ==="
        echo "Orb ID: ${orb_id}"
        echo "Keypath: ${keypath}"

        local json_output="${keypath}/auth.keys.json"
        make_dry_run_json "${orb_id}" "${keypath}" > "${json_output}"
        echo "JSON written to: ${json_output}"
        exit 0
    fi

    echo "Getting Cloudflared access token..."
    local cf_token
    cf_token="$(get_cloudflared_token "${domain}")"

    if [[ ${short} -eq 0 ]]; then
        local cert_payload
        cert_payload="$(
            make_certificate_payload \
                "${orb_id}" \
                "${keypath}/f0000013.cert"
        )"

        post_json \
            "https://${domain}/api/v1/certificate" \
            "${bearer}" \
            "${cf_token}" \
            "${cert_payload}"
    fi

    local signup_payload
    signup_payload="$(
        make_key_payload \
            "${orb_id}" \
            "signup" \
            "${keypath}/sss_70000002_0002_0040.bin" \
            "${keypath}/70000002.signature.raw" \
            "${keypath}/70000002.extra.raw"
    )"

    post_json \
        "https://${domain}/api/v1/key" \
        "${bearer}" \
        "${cf_token}" \
        "${signup_payload}"

    local attestation_payload
    attestation_payload="$(
        make_key_payload \
            "${orb_id}" \
            "attestation" \
            "${keypath}/sss_70000001_0002_0040.bin" \
            "${keypath}/70000001.signature.raw" \
            "${keypath}/70000001.extra.raw"
    )"

    post_json \
        "https://${domain}/api/v1/key" \
        "${bearer}" \
        "${cf_token}" \
        "${attestation_payload}"

    local chipid_payload
    chipid_payload="$(
        make_chipid_payload \
            "${orb_id}" \
            "${keypath}/7fff0206.chip_id.raw" \
            "${keypath}/7fff0206.signature.raw" \
            "${keypath}/7fff0206.extra.raw"
    )"

    post_json \
        "https://${domain}/api/v1/key" \
        "${bearer}" \
        "${cf_token}" \
        "${chipid_payload}"
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    main "$@"
fi
