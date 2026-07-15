# Edit this configuration file to define what should be installed on
# your system.  Help is available in the configuration.nix(5) man page
# and in the NixOS manual (accessible by running ‘nixos-help’).

{
  config,
  pkgs,
  lib,
  ...
}:
{
  imports = [
    # Include the results of the hardware scan.
    ./hardware-configuration.nix
    ../nixos-common.nix
    ../hil-common.nix
  ];

  worldcoin.orbPlatform = "mini";

  services.udev.packages = [ pkgs.android-udev-rules ];

  # qdl-rs/qramdump for flashing Qualcomm SoCs in EDL/QDL mode over USB. Same
  # `plugdev` USB access above covers the raw usbfs nodes it needs.
  environment.systemPackages = [
    (pkgs.callPackage ../../packages/qdl-rs.nix { })
    pkgs.android-tools
  ];

  worldcoin.jenkinsAgent = {
    enable = true;
    url = "https://jenkins.worldcoin.dev";
    #   /etc/worldcoin/secrets/jenkins-cf-access-client-id
    #   /etc/worldcoin/secrets/jenkins-cf-access-client-secret
    cloudflareAccess.enable = true;
  };

  worldcoin.extraPythonPackages = with pkgs.python312Packages; [
    boto3
    pyudev
  ];
}
