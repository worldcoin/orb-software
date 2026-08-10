# WARNING: This is a BOOTSTRAP copy from worldcoin-hil-sf-0. Before the first
# `nixos-rebuild switch` on the real hardware, regenerate this file on the
# machine itself with:
#     nixos-generate-config --show-hardware-config > hardware-configuration.nix
# and commit the result, so disk/filesystem/interface details match the box.
{
  config,
  lib,
  pkgs,
  modulesPath,
  ...
}:

{
  imports = [ (modulesPath + "/installer/scan/not-detected.nix") ];

  boot.initrd.availableKernelModules = [
    "xhci_pci"
    "thunderbolt"
    "ahci"
    "nvme"
    "uas"
    "sd_mod"
  ];
  boot.initrd.kernelModules = [ ];
  boot.kernelModules = [ "kvm-intel" ];
  boot.extraModulePackages = [ ];

  swapDevices = [ ];

  networking.useDHCP = lib.mkDefault true;

  hardware.cpu.intel.updateMicrocode = lib.mkDefault config.hardware.enableRedistributableFirmware;
}
