#!/usr/bin/env bash
# This script runs a command (or $SHELL if none is provided) in the dev container.
# If the dev container isn't running yet, it will start it up.

set -Eeuxo pipefail

# We do this to be agnostic to cwd when we invoke the script.
TOPLEVEL_DIR=$(dirname "$0")/../

# Figure out what the command we will execute will be, store it in CMD.
if [ "$#" -eq "0" ]; then
	CMD="/usr/bin/env "$(basename $SHELL)""
else
	CMD="$@"
fi

# `devcontainer up` creates and starts the container or reuses the existing one.
# Sed extracts the container ID from the stdout of the prior command.
CONTAINER_ID=$(devcontainer up --workspace-folder "$TOPLEVEL_DIR" | sed 's/.*"containerId":"\([^"]*\)".*/\1/')

# Actually execute CMD. We do this instead of devcontainer exec because the
# latter caused issues with TUIs like neovim, whereas docker exec does not seem
# to have these issues.
docker exec -it -w /workspaces/orb-software "$CONTAINER_ID" $CMD
