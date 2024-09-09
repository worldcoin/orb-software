{
  description = "orb-software flake";
  inputs = {
    # Different versions of nixpkgs
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-24.05";
    nixpkgs-unstable.url = "github:NixOS/nixpkgs/nixos-unstable";
    nixpkgs-23_11.url = "github:NixOS/nixpkgs/nixos-23.11";
    # Provides eachDefaultSystem and other utility functions
    flake-utils.url = "github:numtide/flake-utils";
    # Replacement for rustup
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    # Manages dotfiles and home environment
    home-manager = {
      url = "github:nix-community/home-manager/release-24.05";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    # Replaces the need to have a git submodule.
    seekSdk = {
      url = "github:worldcoin/seek-thermal-sdk";
      flake = false;
    };
  };

  outputs = inputs:
    let
      # Used for conveniently accessing nixpkgs on different platforms.
      # We instantiate this once here, and then use it in various other places.
      p = (import ./nix/packages/nixpkgs.nix { inherit inputs; });
      # Creates all of the nixos machines for the flake.
      machines = (import nix/machines/flake-outputs.nix { inherit p inputs; });
      # Creates a `nix develop` shell for every host platform.
      devShells = (import nix/shells/flake-outputs.nix { inherit inputs; instantiatedPkgs = p; });
    in

    # The `//` operators takes the union of its two operands. So we are combining
      # multiple attribute sets into one final, big flake.
    devShells // machines;
}
