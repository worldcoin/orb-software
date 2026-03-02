{
  config,
  pkgs,
  lib,
  ...
}:
let
  cfg = config.orb.hil;
in
{
  options.orb.hil.enable = lib.mkEnableOption "orb-hil package";

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [
      (pkgs.callPackage ../packages/orb-hil.nix { })
    ];
  };
}
