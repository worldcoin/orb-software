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
}
