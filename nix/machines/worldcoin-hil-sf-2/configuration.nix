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
    ../orb-mini-runner.nix
  ];

  worldcoin.orbId = "sf2mini1";
  worldcoin.orbPlatform = "mini";

  environment.etc."worldcoin/orb.yaml" = {
    text = ''
      orb_id: ${config.worldcoin.orbId}
      platform: ${config.worldcoin.orbPlatform}
    '';
    mode = "0644";
  };

  worldcoin.jenkinsAgent = {
    enable = true;
    url = "https://jenkins.worldcoin.dev";
    #   /etc/worldcoin/secrets/jenkins-cf-access-client-id
    #   /etc/worldcoin/secrets/jenkins-cf-access-client-secret
    cloudflareAccess.enable = true;
  };

}
