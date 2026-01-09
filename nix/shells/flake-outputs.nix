# Sets up the developer shell for the repo, which provides all of the build
# tools and dependencies for building and debugging code.
#
# It gets directly combined with the the toplevel flake.nix
{ inputs, instantiatedPkgs }:
let
  inherit (inputs) flake-utils;
  lib = inputs.nixpkgs.lib;
  tegraBashFHS = import ./tegra-bash.nix { pkgs = instantiatedPkgs.x86_64-linux; };
  nfsboot = import ./nfsboot.nix { pkgs = instantiatedPkgs.x86_64-linux; };

  a = {
    # Used like a dev shell, but only for flashing.
    packages."x86_64-linux"."tegra-bash" = tegraBashFHS;
    devShells.x86_64-linux.nfsboot = nfsboot;
  };
  b = flake-utils.lib.eachDefaultSystem (
    system:
    let
      nativePkgs = instantiatedPkgs.${system};
      mainShell = import ./development.nix { inherit inputs system instantiatedPkgs; };
    in
    mainShell
    // {
      # Lets you type `nix fmt` to format the flake.
      formatter = nativePkgs.nixfmt-tree;
    }
  );
in
lib.recursiveUpdate a b
