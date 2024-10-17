#!/usr/bin/env bash

# check flags passed to the script
# `--mcu-update` flag run a test on the Orb to check if the MCU update works
# `--no-run` flag skips the test
# `--help` flag shows the help
# `--version` flag shows the version

VERSION="v0.1.0"

case $1 in
  --mcu-update ) source tools/mcu-update-test.sh && test_mcu_update;;
  --version ) echo "update-agent test script" && echo ${VERSION}; exit 0;;
  * ) echo "Usage: $0 [--mcu-update] [--help] [--version]"; exit 0;;
esac
