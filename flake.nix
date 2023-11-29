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
        alsa = {
          cross = pkgsCross.alsaLib;
          native = if pkgs.stdenv.isLinux then pkgs.alsaLib else null;
        };
        sodium = {
          cross = pkgsCross.libsodium;
          native = pkgs.libsodium;
        };
        openssl = {
          cross = pkgsCross.openssl;
          native = pkgs.openssl;
        };

        seekSdkPath = seekSdk + "/Seek_Thermal_SDK_4.1.0.0";
        # Gets the same rust toolchain that rustup would have used.
        # Note: You don't *have* to do the build with `nix build`,
        # you can still `cargo zigbuild`.
        rustToolchain = fenix.packages.${system}.fromToolchainFile {
          file = ./rust-toolchain.toml;
          sha256 = "sha256-rLP8+fTxnPHoR96ZJiCa/5Ans1OojI7MLsmSqR2ip8o=";
        };
        rustPlatform = pkgs.makeRustPlatform {
          inherit (rustToolchain) cargo rustc;
        };
        llvm = pkgs.llvmPackages;
        crossLibc = let cc = pkgsCross.stdenv.cc; in
          rec {
            package = cc.libc;
            headers = "${package.dev}/include";
          };
        macFrameworks = with pkgs.darwin.apple_sdk.frameworks; [ SystemConfiguration AudioUnit ];


        # Set PKG_CONFIG_PATH for the cross-compiled libraries
        # rust's `pkg-config` build script will prioritize env vars
        # suffixed with the target artchitecture.
        pkgConfigPath = {
          native = pkgs.lib.concatStringsSep ":" ([
            "${sodium.native.dev}/lib/pkgconfig"
            "${openssl.native.dev}/lib/pkgconfig"
          ] ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
            "${alsa.native.dev}/lib/pkgconfig"
          ]);
          cross = pkgs.lib.concatStringsSep ":" [
            "${alsa.cross.dev}/lib/pkgconfig"
            "${sodium.cross.dev}/lib/pkgconfig"
            "${openssl.cross.dev}/lib/pkgconfig"
          ];
        };
      in
      # See https://nixos.wiki/wiki/Flakes#Output_schema
      {
        # Everything in here becomes your shell (nix develop)
        devShells.default = pkgs.mkShell.override { stdenv = pkgs.clangStdenv; } {
          nativeBuildInputs = [
            # For some reason, if we put this in `buildInputs`, nix's wrapper for
            # `pkg-config` will override the `PKG_CONFIG_PATH` env var, which
            # messes up rust's `pkg-config` build script. Keeping it in
            # `nativeBuildInputs` avoids this problem, and is also likely more correct.
            pkgs.pkg-config
          ];

		  # Nix makes the following list of dependencies available to the development
		  # environment.
          buildInputs = [
            # Needed for cargo zigbuild
            pkgs.zig
            pkgs.cargo-zigbuild
            # Useful
            pkgs.cargo-deny
            pkgs.cargo-expand
            pkgs.cargo-binutils
            pkgs.protobuf

            rustToolchain
            rustPlatform.bindgenHook # Configures bindgen to use nix clang
            # This is missing on mac m1 nix, for some reason.
            # see https://stackoverflow.com/a/69732679
            pkgs.libiconv
            # This strikes the happy balance of having the headers available for
            # us when we try to target that platform, without needing to fully
            # rely on the toolchain for cross compilation (we do cross compilation
            # with cargo-zigbuild).
            crossLibc.headers
          ] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
            macFrameworks
          ];

          # The following sets up environment variables for the shell. These are used
          # by the build.rs build scripts of the rust crates.
          shellHook = ''
            export SEEK_SDK_PATH="${seekSdkPath}";
            export PKG_CONFIG_PATH_aarch64_unknown_linux_gnu="${pkgConfigPath.cross}";
            export PKG_CONFIG_PATH="${pkgConfigPath.native}";
            export PKG_CONFIG_ALLOW_CROSS=1;
          '';
        };
        # This formats the nix files, not the rest of the repo.
        formatter = pkgs.nixpkgs-fmt;
      }
    );
}
