# Returns an attrset of nixpkgs for each platform, aka system.
{ inputs }:
let
  inherit (inputs.flake-utils.lib) defaultSystems;
  mkPkgs = (
    system:
    import inputs.nixpkgs {
      inherit system;
      # Overlays modify nixpkgs with new packages.
      # See https://nixos.wiki/wiki/Overlays
      overlays = [
        ((import ../overlays/unstable.nix) { inherit inputs; })
        ((import ../overlays/nixpkgs-23_11.nix) { inherit inputs; })
        (import ../overlays/lz4c.nix)
        (import ../overlays/bacon.nix)
      ];
      # Configuration for nixpkgs.
      config = {
        allowUnfree = true;
        # Accepts the Android SDK/NDK terms of service, required to build
        # `pkgs.androidenv.composeAndroidPackages` (used by
        # nix/packages/android-ndk.nix for cross-compiling to Android).
        # See https://developer.android.com/studio/terms
        android_sdk.accept_license = true;
      };
      flake = abort "this should be specified in nixos modules, its inert here";
    }
  );
  # I hate functional programming 😠
  # Creates an attrset of `{ system = (mkPkgs system)}`
in
builtins.listToAttrs (
  builtins.map (s: {
    name = "${s}";
    value = mkPkgs s;
  }) defaultSystems
)
