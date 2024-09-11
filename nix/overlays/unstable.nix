# This overlay gives us access to nixpkgs unstable.
{ inputs, ... }:
final: _prev: {
  unstable = import inputs.nixpkgs-unstable {
    system = final.system;
    config.allowUnfree = true; # TODO: Use config from original
  };
}
