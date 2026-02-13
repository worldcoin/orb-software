#!/usr/bin/env bash
set -euo pipefail

DEVICE="/dev/hidraw2"

usage() {
  echo "Usage: $0 on|off <1|2|all>"
  exit 1
}

cmd="${1:-}"
relay="${2:-}"
hold="${3:-5}"
[[ "$cmd" == "on" || "$cmd" == "off" || "$cmd" == "on-off" ]] || usage
[[ -n "$relay" ]] || usage

if [[ ! -w "$DEVICE" ]]; then
  echo "Error: cannot write to $DEVICE"
  exit 1
fi

# opcode: ON = 0xFF, OFF = 0xFD
if [[ "$cmd" == "on" ]]  || [[ "$cmd" == "on-off" ]]; then
  opcode='\xFF'
else
  opcode='\xFD'
fi

write_relay() {
  local mask="$1"
  printf "\x00$opcode$(printf '\\x%02X' "$mask")\x00\x00\x00\x00\x00" \
    > "$DEVICE"
}

case "$relay" in
  1)
    write_relay 1   # 0x01
    ;;
  2)
    write_relay 2   # 0x02
    if [[ "$cmd" == "on-off" ]]; then
      sleep $hold
      opcode='\xFD'
      write_relay 2
    fi

    ;;
  all)
    write_relay 1
    write_relay 2
    if [[ "$cmd" == "on-off" ]]; then
      sleep $hold
      opcode='\xFD'
      write_relay 1
      write_relay 2
    fi

    ;;
  *)
    usage
    ;;
esac

