# The Android NDK's standalone Clang-based cross-compilation toolchain, used
# to link Rust crates for Android targets (see the `targets` list in
# ../../rust-toolchain.toml).
{ pkgs }:
let
  # Pinned explicitly (rather than "latest") so bumping nixpkgs doesn't
  # silently change the NDK version, and therefore the linker/output of
  # Android builds, out from under us.
  ndkVersion = "28.2.13676358"; # NDK r28c

  apiLevel = 35;

  androidComposition = pkgs.androidenv.composeAndroidPackages {
    ndkVersions = [ ndkVersion ];
    includeNDK = true;
    includeEmulator = false;
    includeSystemImages = false;
    includeSources = false;
    includeExtras = [ ];
    abiVersions = [ "arm64-v8a" ];
    platformVersions = [ ];
    buildToolsVersions = [ ];
  };

  ndkRoot = "${androidComposition.androidsdk}/libexec/android-sdk/ndk-bundle";
  hostTag = if pkgs.stdenv.hostPlatform.isDarwin then "darwin-x86_64" else "linux-x86_64";
  llvmBin = "${ndkRoot}/toolchains/llvm/prebuilt/${hostTag}/bin";
in
{
  inherit ndkRoot apiLevel;

  cc = "${llvmBin}/aarch64-linux-android${toString apiLevel}-clang";
  cxx = "${llvmBin}/aarch64-linux-android${toString apiLevel}-clang++";
  ar = "${llvmBin}/llvm-ar";
  ranlib = "${llvmBin}/llvm-ranlib";

  # libsodium cross-built for Android via nixpkgs' own NDK cross stdenv, so
  # crates that need it via pkg-config (e.g. `alkali`'s `use-pkg-config`
  # feature in `deps-tests`) can find it - libsodium-sys-stable's own vendored
  # build script doesn't cross-compile.
  libsodium = pkgs.pkgsCross.aarch64-android-prebuilt.libsodium;

  # Same idea for OpenSSL - lets `openssl-sys` find it via pkg-config instead
  # of building it from source itself (the `vendored` feature), which is slow
  # on every clean build. `no-ktls` works around openssl's kernel-TLS code
  # assuming glibc socket headers that Android's Bionic libc doesn't have.
  openssl = pkgs.pkgsCross.aarch64-android-prebuilt.openssl.overrideAttrs (old: {
    configureFlags = (old.configureFlags or [ ]) ++ [ "no-ktls" ];
  });
}
