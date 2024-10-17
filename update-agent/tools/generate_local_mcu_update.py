#!/usr/bin/env python3

import getopt
import hashlib
import json
import os.path
import sys
import os


def main(argv):
    help = "Usage: {} -m path/to/main.bin -s path/to/security.bin -v current_versions.json -c [a-b] \r\nTo be run on Jetson.".format(
        argv[0]
    )

    if len(argv) < 2:
        print(help)
        quit()

    main_app_update_path = None
    sec_app_update_path = None
    versions_path = "versions.json"
    versions = None
    current_slot = None
    update = dict()

    try:
        opts, args = getopt.getopt(
            argv[1:], "m:s:v:c:", ["main=", "sec=", "versions=", "current-slot="]
        )

        if opts != None:
            for opt, arg in opts:
                if opt in ["-m", "--main"]:
                    main_app_update_path = arg
                elif opt in ["-s", "--sec"]:
                    sec_app_update_path = arg
                elif opt in ["-v", "--versions"]:
                    versions_path = arg
                elif opt in ["-c", "--current-slot"]:
                    current_slot = arg
    except getopt.GetoptError:
        print(help)
        exit(1)

    if current_slot is None:
        print(
            "Error: current slot not specified. Please specify the current slot number with -c $(sudo get-slot)"
        )
        exit(1)
    else:
        print("Current slot: {}".format(current_slot))

    if main_app_update_path is None and sec_app_update_path is None:
        print(help)
        print("Error: at least one binary is expected")
        exit(1)

    if (
        main_app_update_path is not None and not os.path.exists(main_app_update_path)
    ) and (sec_app_update_path is not None and not os.path.exists(sec_app_update_path)):
        print("Error: wrong path to MCU binary")
        exit(1)

    try:
        with open(versions_path) as v:
            versions = json.load(v)
    except Exception as e:
        print(f"Error: unable to load versions: {e}")
        exit(1)

    update["update"] = True
    # consider local updates as dirty
    update["version"] = "dirty"
    sources = dict()
    abs_paths = dict()
    if main_app_update_path is not None and os.path.exists(main_app_update_path):
        sources["mainboard"] = dict()
        abs_paths["mainboard"] = os.path.abspath(main_app_update_path)
    if sec_app_update_path is not None and os.path.exists(sec_app_update_path):
        sources["security"] = dict()
        abs_paths["security"] = os.path.abspath(sec_app_update_path)
    update["sources"] = sources

    manifest = dict()
    manifest["magic"] = "W0r1dC01n"
    manifest["type"] = "normal"
    manifest["components"] = []

    for mcu in ["mainboard", "security"]:
        if mcu in sources:
            component = dict()
            component["name"] = mcu

            if (
                f"slot_{current_slot}" not in versions
                or "mcu" not in versions[f"slot_{current_slot}"]
            ):
                component["version-assert"] = ""
            else:
                component["version-assert"] = versions[f"slot_{current_slot}"]["mcu"][
                    f"{mcu}"
                ]

            component["version"] = None  # initialize with None
            component["size"] = os.path.getsize(
                abs_paths[mcu]
            )  # size of installed component
            component["installation_phase"] = "normal"  # `normal` or `recovery`

            # open file to compute sha256 and get firmware version from the binary file
            sha256_hash = hashlib.sha256()
            with open(abs_paths[mcu], "rb") as f:
                # we can load entire file as the file won't exceed 250kB
                byte_block = f.read()
                if byte_block:
                    sha256_hash.update(byte_block)
                    # the binary version is written into the binary at offset 20
                    if (
                        component["version"] is None
                        and int(byte_block[0]) == 0x3D
                        and int(byte_block[1]) == 0xB8
                    ):
                        component["version"] = (
                            f"{int(byte_block[20])}.{int(byte_block[21])}.{int(byte_block[22])}"
                        )
                        print(f'Binary for {mcu}, version: {component["version"]}')
                component["hash"] = sha256_hash.hexdigest()

            manifest["components"].append(component)

            # now XZ compress the file
            os.system("xz -zkf " + abs_paths[mcu])
            sources[mcu]["url"] = abs_paths[mcu] + ".xz"
            sources[mcu]["size"] = os.path.getsize(sources[mcu]["url"])
            sources[mcu]["name"] = mcu
            sources[mcu]["mime_type"] = "application/x-xz"

            # compute hash of compressed binary
            sha256_hash = hashlib.sha256()
            with open(sources[mcu]["url"], "rb") as f:
                # we can load entire file as the file won't exceed 250kB
                byte_block = f.read()
                if byte_block:
                    sha256_hash.update(byte_block)
                sources[mcu]["hash"] = sha256_hash.hexdigest()

            # make sure url starts with file://
            if not sources[mcu]["url"].__contains__("file://"):
                sources[mcu]["url"] = "file://" + sources[mcu]["url"]

    update["manifest"] = manifest
    update["signature"] = ""
    update["manifest-sig"] = ""
    update["system_components"] = json.loads(
        '{"security":{"type":"can","value":{"address":2,"bus":"can0",'
        '"redundancy":"redundant"}},"mainboard":{"type":"can",'
        '"value":{"address":1,"bus":"can0","redundancy":"redundant"}}}'
    )

    print(json.dumps(update))
    with open("response.json", "w") as f:
        f.write(json.dumps(update))


if __name__ == "__main__":
    main(sys.argv)
