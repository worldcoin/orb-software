#!/usr/bin/env nu
# Allows running tests inside of podman.
# If nu shell is not there, install it: 'cargo install --locked nu'

def main [prog, args] {
	let absolute_path = ($prog | path expand)

	(podman run
	 --rm
	 -v $"($absolute_path):/mnt/program:Z"
	 -w /mnt
	 -e RUST_BACKTRACE
	 -it fedora:latest
	 /mnt/program)
}
