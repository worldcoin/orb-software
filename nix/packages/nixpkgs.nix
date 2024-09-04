# Returns an attrset of nixpkgs for each platform, aka system.
{ inputs }:
let
  inherit (inputs.flake-utils.lib) defaultSystems;
  mkPkgs = (system: import inputs.nixpkgs {
    inherit system;
    # Overlays modify nixpkgs with new packages.
    # See https://nixos.wiki/wiki/Overlays
    overlays = [
      ((import ../overlays/unstable.nix) { inherit inputs; })
      ((import ../overlays/nixpkgs-23_11.nix) { inherit inputs; })
    ];
    # Configuration for nixpkgs.
    config = {
      allowUnfree = true;
    };
  });
in
# I hate functional programming 😠
  # Creates an attrset of `{ system = (mkPkgs system)}`
builtins.listToAttrs (
  builtins.map (s: { name = "${s}"; value = mkPkgs s; }) defaultSystems
)
