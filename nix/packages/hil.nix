# Packages for the HIL.
# Note: orb-hil is installed systemwide via hil-common.nix
{ pkgs, ... }:
with pkgs;
[
  # HIL Specific
  awscli2
  cloudflared
  git
  gnutar
  picocom
  probe-rs
  ripgrep
  usbutils

  # Build tools
  cmake
  file
  gnumake
  ninja
  zig
]
