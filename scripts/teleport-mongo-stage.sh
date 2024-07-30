#!/usr/bin/env bash

set -o errexit   # abort on nonzero exitstatus
set -o errtrace  # pass ERR trap down to functions, substitutions, etc
set -o nounset   # abort on unbound variable
set -o pipefail  # don't hide errors within pipes

# Usage
# This script is intended to be used along with a Mongo GUY Client, like Compass
# 1. Run this script
# 2. Copy the mongo url printed at the end, e.g. mongodb://localhost:37483/?serverSelectionTimeoutMS=5000
# 3. Leave this terminal open.
# 4. Open Compass and paste the url in the connection field
# 5. In "Advanced", click "Direct Connection"
# 6. Click "Connect"

tsh db login --db-user arn:aws:iam::510867353226:role/developer-read-write --proxy=teleport.worldcoin.dev:443 mongo-atlas-orb-stage

tsh proxy db --tunnel mongo-atlas-orb-stage
