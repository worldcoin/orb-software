#!/bin/bash

declare -g DEVICE=/dev/video1
if [[ -z "${OUTPUT-}" ]]; then
        declare -g OUTPUT="${1:-"test_3280x2464.raw"}"
fi

: ${EXPOSURE:="33300"}
: ${GAIN:="20"}

echo "Setting exposure: ${EXPOSURE}"
echo "Setting gain    : ${GAIN}"

v4l2-ctl -d ${DEVICE} \
        --set-fmt-video=width=3280,height=2464,pixelformat=RG10 \
        --set-ctrl=override_enable=1,bypass_mode=0 \
        --set-ctrl=gain=${GAIN},exposure=${EXPOSURE} \
        --stream-mmap --stream-count=1 --stream-to=${OUTPUT} \
        --set-ctrl=preferred_stride=32 \
        --verbose #\
        #--stream-skip=1

v4l2-ctl -d ${DEVICE} --list-ctrls
