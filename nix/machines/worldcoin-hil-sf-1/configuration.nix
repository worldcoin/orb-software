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

  # This HIL doubles as a Jenkins agent (in addition to the GitHub Actions
  # runner set up by hil-common.nix). It runs the HIL test stage of the
  # build_t824 pipeline.
  #
  # Before this connects you must, on the Jenkins controller:
  #   1. Manage Jenkins -> Nodes -> New Node -> "worldcoin-hil-sf-1"
  #      (Permanent Agent, launch method "connect agent to controller").
  #   2. Add label `worldcoin-hil-sf-1` (this is what the Jenkinsfile targets).
  #   3. Copy the node's secret into /etc/worldcoin/secrets/jenkins-agent-secret
  #      on this machine (root-owned, mode 0400).
  # This machine is Jenkins-only: skip the GitHub Actions runner that
  # hil-common.nix sets up for every other HIL.
  worldcoin.githubRunner.enable = false;

  # ADB/fastboot access to Android-based test hardware attached to this HIL.
  # Ships the udev rules from the Debian `android-sdk-platform-tools-common`
  # package; `jenkins-agent-user` already gets `plugdev` via jenkins-agent.nix's
  # default extraGroups. NixOS reloads/retriggers udev rules automatically on
  # `nixos-rebuild switch`, so no manual `udevadm control --reload-rules` step
  # is needed.
  services.udev.packages = [ pkgs.android-udev-rules ];

  # qdl-rs/qramdump for flashing Qualcomm SoCs in EDL/QDL mode over USB. Same
  # `plugdev` USB access above covers the raw usbfs nodes it needs.
  environment.systemPackages = [ (pkgs.callPackage ../../packages/qdl-rs.nix { }) ];

  worldcoin.jenkinsAgent = {
    enable = true;
    url = "https://jenkins.worldcoin.dev";
    # jenkins.worldcoin.dev is behind Cloudflare Access; the agent authenticates
    # with a service token. Provision these two files on the machine (0400, root):
    #   /etc/worldcoin/secrets/jenkins-cf-access-client-id
    #   /etc/worldcoin/secrets/jenkins-cf-access-client-secret
    cloudflareAccess.enable = true;
  };
}
