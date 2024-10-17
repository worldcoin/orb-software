#!/usr/bin/env bash

GREEN='\033[0;32m'
NC='\033[0m' # No Color

TARGET_PATH="/mnt/scratch"

test_mcu_update () {
	# Allows us to read user input below, assigns stdin to keyboard
	exec < /dev/tty
	echo -e "Do you want to run the MCU update test? ${GREEN}[Y/n]${NC} (you will be guided 🙂)"
	read -p "> " tested
	case $tested in
  n ) exit 0;;
	* ) ;;
	esac

  echo ""
  echo -e "${GREEN}Building the test...${NC}"
	echo "👉 In the meantime: make sure to scp a main microcontroller binary to Jetson's $TARGET_PATH/app_mcu_main_test.bin"
	echo "Find this binary in https://github.com/worldcoin/orb-mcu-firmware/releases"
	echo "Then in artifacts/orb/main_board/app/build/zephyr/app_mcu_main_*"
	echo ""
	echo "👉 Type a key to continue"
	read dummy
	cargo clean --target-dir target/aarch64-unknown-linux-gnu/debug/deps/
	cargo-zigbuild test --no-run --target aarch64-unknown-linux-gnu.2.27 --features can-update-test --lib
	# check last command exit code
	if [ $? -ne 0 ]; then
    echo "🚨 Failed to compile test"
    exit 1;
  fi
	echo "Type Orb's IP:"
	read -p "> " ip
	filepath=$(ls -t target/aarch64-unknown-linux-gnu/debug/deps/update_agent* | head -1)
	filename=$(basename $filepath)
	echo "Sending $filepath to $ip"
	scp ${filepath} worldcoin@${ip}:$TARGET_PATH/${filename}
	ssh worldcoin@${ip} "$TARGET_PATH/$filename"
	exit 1;
}
