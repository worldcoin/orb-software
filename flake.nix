{
  description = "orb-software flake";
  inputs = {
    # Worlds largest repository of linux software
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-23.11";
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
        p = {
          # The platform that you are running nix on and building from
          native = nixpkgs.legacyPackages.${system};
          # The other platforms we support for cross compilation
          arm-linux = nixpkgs.legacyPackages.aarch64-linux;
          x86-linux = nixpkgs.legacyPackages.x86_64-linux;
          arm-macos = nixpkgs.legacyPackages.aarch64-darwin;
          x86-macos = nixpkgs.legacyPackages.x86_64-darwin;
        };

        # p.native.lib.asserts.assertOneOf "system" system ["aarch64-linux"]; 

        seekSdkPath = seekSdk + "/Seek_Thermal_SDK_4.1.0.0";
        # Gets the same rust toolchain that rustup would have used.
        # Note: You don't *have* to do the build with `nix build`,
        # you can still `cargo zigbuild`.
        rustToolchain = fenix.packages.${system}.fromToolchainFile {
          file = ./rust-toolchain.toml;
          sha256 = "sha256-Ngiz76YP4HTY75GGdH2P+APE/DEIx2R/Dn+BwwOyzZU=";
        };
        rustPlatform = p.native.makeRustPlatform {
          inherit (rustToolchain) cargo rustc;
        };
        macFrameworks = with p.native.darwin.apple_sdk.frameworks; [
          SystemConfiguration
          AudioUnit
        ];

        # Set PKG_CONFIG_PATH for the cross-compiled libraries
        # rust's `pkg-config` build script will prioritize env vars
        # suffixed with the target artchitecture.
        makePkgConfigPath = p: p.lib.concatStringsSep ":" ([
          "${p.libsodium.dev}/lib/pkgconfig"
          "${p.openssl.dev}/lib/pkgconfig"
        ] ++ p.lib.lists.optionals p.stdenv.isLinux [
          "${p.alsaLib.dev}/lib/pkgconfig"
        ]);
        pkgConfigPath = {
          native = makePkgConfigPath p.native;
          arm-linux = makePkgConfigPath p.arm-linux;
          x86-linux = makePkgConfigPath p.x86-linux;
          arm-macos = makePkgConfigPath p.arm-macos;
          x86-macos = makePkgConfigPath p.x86-macos;
        };
      in
      # See https://nixos.wiki/wiki/Flakes#Output_schema
      {
        # Everything in here becomes your shell (nix develop)
        devShells.default = p.native.mkShell.override
          {
            stdenv = p.native.clangStdenv;
          }
          {
            # Nix makes the following list of dependencies available to the development
            # environment.
            buildInputs = (with p.native; [
              mdbook # Generates site for docs
              protobuf # Needed for orb-messages and other protobuf dependencies
              black # Python autoformatter
              cargo-binutils # Contains common native development utilities
              cargo-deb # Generates .deb packages for orb-os
              cargo-deny # Checks licenses and security advisories
              cargo-expand # Useful for inspecting macros
              cargo-zigbuild # Used to cross compile rust
              nixpkgs-fmt # Nix autoformatter
              python3
              zig # Needed for cargo zigbuild

              # This is missing on mac m1 nix, for some reason.
              # see https://stackoverflow.com/a/69732679
              libiconv

              # Used by various rust build scripts to find system libs
              # Note that this is the unwrapped version of pkg-config. By default,
              # nix wraps pkg-config with a script that replaces the PKG_CONFIG_PATH
              # with the proper settings for cross compilation. We already set these
              # env variables ourselves and don't want nix overwriting them, so we
              # use the unwrapped version.
              pkg-config-unwrapped
            ]) ++ [
              rustToolchain
              rustPlatform.bindgenHook # Configures bindgen to use nix clang
            ] ++ p.native.lib.lists.optionals p.native.stdenv.isDarwin [
              macFrameworks
            ];

            # The following sets up environment variables for the shell. These are used
            # by the build.rs build scripts of the rust crates.
            shellHook = ''
              export SEEK_SDK_PATH="${seekSdkPath}";
              export PKG_CONFIG_ALLOW_CROSS=1;
              export PKG_CONFIG_PATH_aarch64_unknown_linux_gnu="${pkgConfigPath.arm-linux}";
              export PKG_CONFIG_PATH_x86_64_unknown_linux_gnu="${pkgConfigPath.x86-linux}";
              export PKG_CONFIG_PATH_aarch64_apple_darwin="${pkgConfigPath.arm-macos}";
              export PKG_CONFIG_PATH_x86_64_apple_darwin="${pkgConfigPath.x86-macos}";
            '';
          };
        devShells.tegra-flash = (p.native.buildFHSEnv {
          name = "tegra-env";
          targetPkgs = pkgs: (with pkgs; [
            curl
            lz4
            perl
            udev
          ]);
          runScript = "bash";
        }).env;

        # Lets you type `nix fmt` to format the flake.
        formatter = p.native.nixpkgs-fmt;
      }
    );
}
