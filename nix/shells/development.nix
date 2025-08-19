# Defines the content of the main dev shell for developing in the repo.
# 
# This gets combined with `flake-outputs.nix`, which itself is combined with the
# toplevel `flake.nix`.
{ fenix, system, instantiatedPkgs, seekSdk }:
let
  p = instantiatedPkgs // {
    native = p.${system};
  };
  seekSdkPath = seekSdk + "/Seek_Thermal_SDK_4.1.0.0";
  # Gets the same rust toolchain that rustup would have used.
  # Note: You don't *have* to do the build with `nix build`,
  # you can still `cargo zigbuild`.
  rustToolchain = fenix.packages.${system}.fromToolchainFile {
    file = ../../rust-toolchain.toml;
    sha256 = "sha256-Qxt8XAuaUR2OMdKbN4u8dBJOhSHxS+uS06Wl9+flVEk=";
  };
  rustPlatform = p.native.makeRustPlatform {
    inherit (rustToolchain) cargo rustc;
  };
  macFrameworks = with p.native.darwin.apple_sdk.frameworks; [
    AppKit
    AudioUnit
    SystemConfiguration
  ];

  # Set PKG_CONFIG_PATH for the cross-compiled libraries
  # rust's `pkg-config` build script will prioritize env vars
  # suffixed with the target artchitecture.
  makePkgConfigPath = p: p.lib.concatStringsSep ":" ([
    "${p.nixpkgs-23_11.glib.dev}/lib/pkgconfig"
    "${p.nixpkgs-23_11.gst_all_1.gst-plugins-base.dev}/lib/pkgconfig"
    "${p.nixpkgs-23_11.gst_all_1.gstreamer.dev}/lib/pkgconfig"
    "${p.nixpkgs-23_11.libsodium.dev}/lib/pkgconfig"
    "${p.nixpkgs-23_11.lzma.dev}/lib/pkgconfig"
    "${p.nixpkgs-23_11.openssl.dev}/lib/pkgconfig"
    "${p.nixpkgs-23_11.squashfs-tools-ng}/lib/pkgconfig"
  ] ++ p.lib.lists.optionals p.stdenv.isLinux [
    "${p.nixpkgs-23_11.alsaLib.dev}/lib/pkgconfig"
    "${p.nixpkgs-23_11.udev.dev}/lib/pkgconfig"
  ]);
  pkgConfigPath = {
    native = makePkgConfigPath p.native;
    aarch64-linux = makePkgConfigPath p.aarch64-linux;
    x86_64-linux = makePkgConfigPath p.x86_64-linux;
    aarch64-darwin = makePkgConfigPath p.aarch64-darwin;
    x86_64-darwin = makePkgConfigPath p.x86_64-darwin;
  };

in
{
  # Everything in here becomes your shell (nix develop)
  devShells.default = p.native.mkShell
    {
      # Nix makes the following list of dependencies available to the development
      # environment.
      buildInputs = (with p.native; [
        bacon # better cargo-watch
        black # Python autoformatter
        cargo-binutils # Contains common native development utilities
        cargo-deb # Generates .deb packages for orb-os
        cargo-expand # Useful for inspecting macros
        cargo-watch # Useful for repeatedly running tests
        cargo-zigbuild # Used to cross compile rust
        dpkg # Used to test outputs of cargo-deb
        git-cliff # Conventional commit based release notes
        mdbook # Generates site for docs
        mdbook-mermaid # Adds mermaid support
        nixpkgs-fmt # Nix autoformatter
        nushell # Cross platform shell for scripts
        protobuf # Needed for orb-messages and other protobuf dependencies
        python3
        squashfsTools # mksquashfs
        sshpass # Needed for orb-software/scripts 
        taplo # toml autoformatter
        unstable.cargo-deny # Checks licenses and security advisories
        zbus-xmlgen # Used by `orb-zbus-proxies`
        zig # Needed for cargo zigbuild

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
        # This is missing on mac m1 nix, for some reason.
        # see https://stackoverflow.com/a/69732679
        p.native.libiconv
      ];

      # The following sets up environment variables for the shell. These are used
      # by the build.rs build scripts of the rust crates.
      shellHook = ''
        export SEEK_SDK_PATH="${seekSdkPath}";
        export PKG_CONFIG_ALLOW_CROSS=1;
        export PKG_CONFIG_PATH_aarch64_unknown_linux_gnu="${pkgConfigPath.aarch64-linux}";
        export PKG_CONFIG_PATH_x86_64_unknown_linux_gnu="${pkgConfigPath.x86_64-linux}";
        export PKG_CONFIG_PATH_aarch64_apple_darwin="${pkgConfigPath.aarch64-darwin}";
        export PKG_CONFIG_PATH_x86_64_apple_darwin="${pkgConfigPath.x86_64-darwin}";
      '';
    };
}
