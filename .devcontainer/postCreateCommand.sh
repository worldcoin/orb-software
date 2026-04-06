#!/usr/bin/env bash

set -Eeuxo pipefail

sudo chown ubuntu target

git config --global --add safe.directory /workspaces/orb-software

# Get direnv to work in the bash scripts
if [ ! -e .envrc ]; then
    cp .envrc.example .envrc # Bootstrap for the user
    direnv allow
fi

if [ -e .devcontainer/postCreateCommand.user.sh ]; then
    .devcontainer/postCreateCommand.user.sh
fi
