{ config, pkgs, modulesPath, ... }:
{
  imports = [
    (modulesPath + "/installer/cd-dvd/installation-cd-minimal.nix")
  ];
  environment.systemPackages = with pkgs; [
    neovim
    parted
    usbutils
  ];
}
