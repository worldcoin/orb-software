# Package definition for qdl-rs / qramdump: Rust tools for talking to
# Qualcomm SoCs in Emergency Download (EDL) mode over the Sahara/Firehose
# protocols, used for QDL flashing.
# Upstream: https://github.com/qualcomm/qdlrs
{ pkgs }:
let
  # Upstream has no tagged releases yet, so pin a commit.
  rev = "bfb733a9900129534c5ba0f35a7d1e8b9fc92392";
  src = pkgs.unstable.fetchFromGitHub {
    owner = "qualcomm";
    repo = "qdlrs";
    inherit rev;
    hash = "sha256-tj8/rGw2UgKszHItlvzXgxfGPBow3W9LWMbcodsiiFY=";
  };
in
# Needs a newer rustc than nixos-25.05 ships (qdlrs uses `is_multiple_of`,
# stabilized in rustc 1.87), so build with nixpkgs-unstable's rustPlatform.
pkgs.unstable.rustPlatform.buildRustPackage {
  pname = "qdl-rs";
  version = "unstable-2026-06-19";
  inherit src;

  cargoLock.lockFile = "${src}/Cargo.lock";

  meta = with pkgs.lib; {
    description = "Tools for flashing Qualcomm SoCs in EDL/QDL mode (qdl-rs, qramdump)";
    homepage = "https://github.com/qualcomm/qdlrs";
    license = licenses.bsd3;
    mainProgram = "qdl-rs";
    platforms = platforms.linux;
  };
}
