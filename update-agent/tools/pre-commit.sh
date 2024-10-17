#!/usr/bin/env bash

source tools/mcu-update-test.sh

# Ask to perform a test in case can.rs changed
FILES_PATTERN='can.rs'
if git diff --cached --name-only | grep -qE $FILES_PATTERN; then
  echo "⚠️ src/update/can.rs changed, a test is available"
  test_mcu_update
else
  exit 0;
fi
