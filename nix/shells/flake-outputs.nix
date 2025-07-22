# Sets up the developer shell for the repo, which provides all of the build
# tools and dependencies for building and debugging code.
#
# It gets directly combined with the the toplevel flake.nix
{ inputs, instantiatedPkgs }:
let
  inherit (inputs) flake-utils seekSdk fenix;
  tegraBashFHS = import ./tegra-bash.nix { pkgs = instantiatedPkgs.x86_64-linux; };
in
# This helper function is used to more easily abstract
  # over the host platform.
  # See https://github.com/numtide/flake-utils#eachdefaultsystem--system---attrs
{
  # Used like a dev shell, but only for flashing.
  packages."x86_64-linux"."tegra-bash" = tegraBashFHS;
} //
flake-utils.lib.eachDefaultSystem
  (system:
    let
      nativePkgs = instantiatedPkgs.${system};
      mainShell = import ./development.nix { inherit system fenix instantiatedPkgs seekSdk; };
    in
    mainShell //
    {
      # Lets you type `nix fmt` to format the flake.
      formatter = nativePkgs.nixpkgs-fmt;
    }
  )
