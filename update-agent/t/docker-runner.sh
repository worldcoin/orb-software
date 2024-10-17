#!/usr/bin/env bash
# Allows running tests inside docker. Takes care of linux-only dependencies and avoids
# the need to emulate the architecture.

set -o pipefail -eu

PROGRAM=$1; shift

main() {
	local -r absolute_path="$(realpath ${PROGRAM})"

	docker run \
		--rm \
		-v "${absolute_path}:/mnt/program" \
		-w /mnt \
		-e RUST_BACKTRACE \
		-it debian:latest \
		/mnt/program $@
}

main $@
