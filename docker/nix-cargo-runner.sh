#!/usr/bin/env bash
# Allows running tests inside docker. Takes care of linux-only dependencies and avoids
# the need to emulate the architecture.

set -Eeuo pipefail

SELF=$0;
PROGRAM=$1; shift

main() {
	local -r absolute_path="$(realpath ${PROGRAM})"

	if [[ "$(uname -m)" == "arm64" ]]; then
		ARCH="aarch64-linux"
	else
		ARCH="x86_64-linux"
	fi
	IMAGE_TGZ="$(nix build .#containers.${ARCH}.cargo-runner --print-out-paths)"
	docker load < "${IMAGE_TGZ}"
	docker run \
		--rm \
		-v "${absolute_path}:/mnt/program" \
		-w /mnt \
		-e RUST_BACKTRACE \
		-it nix-cargo-runner:latest \
		/mnt/program $@
		# gdb -ex=run -ex=quit --args /mnt/program $@
}

main $@
