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
  #   1. Manage Jenkins -> Nodes -> New Node -> "worldcoin-hil-munich-1"
  #      (Permanent Agent, launch method "connect agent to controller").
  #   2. Add label `worldcoin-hil-munich-1` (this is what the Jenkinsfile targets).
  #   3. Copy the node's secret into /etc/worldcoin/secrets/jenkins-agent-secret
  #      on this machine (root-owned, mode 0400).
  worldcoin.jenkinsAgent = {
    enable = true;
    url = "https://jenkins.worldcoin.dev";
  };
}
