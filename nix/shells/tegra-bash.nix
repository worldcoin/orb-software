# Creates a FHS chroot with all the necessary tools used for flashing an orb.
# NOTE(@thebutlah): AFAICT this is not a dev shell. But it can act like one if
# you make the `runScript` bash, and then run it.
{ pkgs }:
let
  pythonShell = (ps: with ps; [
    pyyaml
    pyserial # just for convenience
    pyftdi # for controlling UART adapter

    # for jtag debugger
    pyocd
    cmsis-pack-manager
    cffi
  ]);
in
pkgs.buildFHSUserEnv {
  name = "tegra-bash";
  targetPkgs = pkgs: (with pkgs; [
    (python3.withPackages pythonShell)
    bun
    curl
    dtc
    gcc
    libxml2
    lz4
    openssl
    perl
    udev
  ]);
  runScript = "bash";
}
