# This gets directly combined with the the toplevel flake.nix
{ inputs, instantiatedPkgs }:
let
  inherit (inputs) flake-utils;
  containerSystems = [
    "x86_64-linux"
    "aarch64-linux"
  ];
  # This helper function is used to more easily abstract
  # over the host platform.
  # See https://github.com/numtide/flake-utils#eachdefaultsystem--system---attrs
in
flake-utils.lib.eachSystem containerSystems (
  system:
  let
    nativePkgs = instantiatedPkgs.${system};
    cargoRunner = import ./cargo-runner.nix {
      inherit system;
      pkgs = nativePkgs;
      lib = inputs.nixpkgs.lib;
    };
  in
  {
    containers.cargo-runner = cargoRunner;
  }
)
