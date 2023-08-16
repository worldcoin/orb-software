{
  description = "orb-software flake";
  inputs = {
    # Worlds largest repository of linux software
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    # Provides eachDefaultSystem and other utility functions
    utils.url = "github:numtide/flake-utils";
    # Replacement for rustup
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    # Replaces the need to have a git submodule.
    seekSdk = {
      url = "github:worldcoin/seek-thermal-sdk";
      flake = false;
    };
  };

  outputs = { self, nixpkgs, utils, fenix, seekSdk }:
    # This helper function is used to more easily abstract
    # over the host platform.
    # See https://github.com/numtide/flake-utils#eachdefaultsystem--system---attrs
    utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        seekSdkPath = seekSdk + "/Seek_Thermal_SDK_4.1.0.0";
        # Gets the same rust toolchain that rustup would have used.
        # Note: You don't *have* to do the build with `nix build`,
        # you can still `cargo zigbuild`.
        rustToolchain = fenix.packages.${system}.fromToolchainFile {
          file = ./rust-toolchain.toml;
          sha256 = "R0F0Risbr74xg9mEYydyebx/z0Wu6HI0/KWwrV30vZo=";
        };
      in
      # See https://nixos.wiki/wiki/Flakes#Output_schema
      {
        # Everything in here becomes your shell (nix develop)
        devShells.default = pkgs.mkShell {
          # Compile-time dependencies
          nativeBuildInputs = [
            rustToolchain
            # This is missing on mac m1 nix, for some reason.
            # see https://stackoverflow.com/a/69732679
            pkgs.libiconv
          ];
          shellHook = ''export SEEK_SDK_PATH="${seekSdkPath}"'';
        };
        # This formats the nix files, not the rest of the repo.
        formatter = pkgs.nixpkgs-fmt;
      }
    );
}
