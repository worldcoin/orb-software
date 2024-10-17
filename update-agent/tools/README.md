# Tools

## Local microcontroller update

Generating a `response.json` to enable microcontroller's update based on local files.

## How-to

An example below:
```shell
# load python script on the Jetson
scp generate_local_mcu_update.py worldcoin@<orb-ip>:/mnt/scratch
# send the public key (from 1Password: "OTA staging key pair")
scp worldcoin-staging-ota-pub.der worldcoin@<orb-ip>:/mnt/scratch
# send MCU firmware binaries taken from orb-mcu-firmware artifacts
scp orb/<main|sec>_board/app/build/zephyr/app_mcu_*.signed.encrypted.bin worldcoin@<orb-ip>:/mnt/scratch

# make script executable
chmod +x generate_local_mcu_update.py

# generate `response.json` for main and security MCU
# use absolute or relative paths
# this will create the compressed files (`.xz`)
./generate_local_mcu_update.py -m app_mcu_main_<version>.signed.encrypted.bin -s app_mcu_sec_<version>.signed.encrypted.bin -v /usr/persistent/versions.json -c $(get-slot)

# run the update
sudo ./update-agent --active-slot "$(get-slot)" --id "$(orb-id)" --nodbus --pubkey worldcoin-staging-ota-pub.der --update response.json
```
