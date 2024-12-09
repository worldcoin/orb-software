#!/usr/bin/env bash

set -Eeuo pipefail

PROGRAM="orb-mcu-util"

if [ -t 0 ] ; then
    # Interactive terminal, print a warning
    echo "$(tput setaf 1)WARNING: this program has been moved to /usr/local/bin/${PROGRAM}, this wrapper will be removed in the next release$(tput sgr0)" >&2
else
    PARENT=$(cat /proc/$PPID/cmdline | tr \\0 \ )
    echo "moved to /usr/local/bin/${PROGRAM}" | systemd-cat -t ${PROGRAM} -p warning
    echo "${PROGRAM} is called by ${PARENT}" | systemd-cat -t ${PROGRAM} -p warning
fi
exec ${PROGRAM} $@
