# The Android NDK's standalone Clang-based cross-compilation toolchain, used
# to link Rust crates for Android targets (see the `targets` list in
# ../../rust-toolchain.toml). We only need the NDK's toolchain, not the rest
# of the Android SDK (build-tools/platform-tools/emulator/etc), so we compose
# a minimal package set that skips all of that.
{ pkgs }:
let
  # Pinned explicitly (rather than "latest") so bumping nixpkgs doesn't
  # silently change the NDK version, and therefore the linker/output of
  # Android builds, out from under us.
  ndkVersion = "28.2.13676358"; # NDK r28c

  # The lowest Android API level the cross-compiled binaries will support.
  # Bump this if a higher floor is ever required; the NDK ships one prebuilt
  # Clang wrapper per API level (e.g. `aarch64-linux-android24-clang`), all
  # from the same toolchain, so this alone is enough to change it.
  apiLevel = 24;

  androidComposition = pkgs.androidenv.composeAndroidPackages {
    ndkVersions = [ ndkVersion ];
    includeNDK = true;
    # We only cross-compile with the NDK's toolchain here, we don't build or
    # run an actual .apk, so skip everything else the SDK would otherwise
    # pull in.
    includeEmulator = false;
    includeSystemImages = false;
    includeSources = false;
    includeExtras = [ ];
    platformVersions = [ ];
    buildToolsVersions = [ ];
  };

  ndkRoot = "${androidComposition.androidsdk}/libexec/android-sdk/ndk-bundle";
  hostTag = if pkgs.stdenv.hostPlatform.isDarwin then "darwin-x86_64" else "linux-x86_64";
  llvmBin = "${ndkRoot}/toolchains/llvm/prebuilt/${hostTag}/bin";
in
{
  inherit ndkRoot apiLevel;

  # Toolchain for aarch64-linux-android (the only Android target this
  # workspace cross-compiles to today - see rust-toolchain.toml). If a
  # second ABI is ever needed, generalize this into a per-triple attrset
  # then.
  cc = "${llvmBin}/aarch64-linux-android${toString apiLevel}-clang";
  ar = "${llvmBin}/llvm-ar";
  ranlib = "${llvmBin}/llvm-ranlib";
}
