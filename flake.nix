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
        pkgsCross = nixpkgs.legacyPackages.aarch64-linux;
        seekSdkPath = seekSdk + "/Seek_Thermal_SDK_4.1.0.0";
        # Gets the same rust toolchain that rustup would have used.
        # Note: You don't *have* to do the build with `nix build`,
        # you can still `cargo zigbuild`.
        rustToolchain = fenix.packages.${system}.fromToolchainFile {
          file = ./rust-toolchain.toml;
          sha256 = "sha256-rLP8+fTxnPHoR96ZJiCa/5Ans1OojI7MLsmSqR2ip8o=";
        };
        llvm = pkgs.llvmPackages;
        crossLibc = let cc = pkgsCross.stdenv.cc; in
          rec {
            package = cc.libc.dev;
            headers = "${package}/include";
          };
        macFrameworks = with pkgs.darwin.apple_sdk.frameworks; [ SystemConfiguration ];
      in
      # See https://nixos.wiki/wiki/Flakes#Output_schema
      {
        # Everything in here becomes your shell (nix develop)
        devShells.default = pkgs.mkShell.override { stdenv = pkgs.clangStdenv; } {
          # Compile-time dependencies
          buildInputs = [
            # Needed for cargo zigbuild
            pkgs.zig
            pkgs.cargo-zigbuild
            # Useful
            pkgs.cargo-deny
            pkgs.cargo-expand
            pkgs.cargo-binutils

            rustToolchain
            # This is missing on mac m1 nix, for some reason.
            # see https://stackoverflow.com/a/69732679
            pkgs.libiconv
            # This strikes the happy balance of having the headers available for
            # us when we try to target that platform, without needing to fully
            # rely on the toolchain for cross compilation (we do cross compilation
            # with cargo-zigbuild).
            crossLibc.headers

			# Native dependencies
			pkgs.openssl
          ] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [macFrameworks];
          shellHook = ''
                        		export SEEK_SDK_PATH="${seekSdkPath}";
            					export LIBCLANG_PATH="${llvm.libclang.lib}/lib";
                        	  '';
        };
        # This formats the nix files, not the rest of the repo.
        formatter = pkgs.nixpkgs-fmt;
      }
    );
}
