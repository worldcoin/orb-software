# Packages for the HIL.
{ pkgs, ... }:
with pkgs;
let
  orb-hil = pkgs.callPackage ./orb-hil.nix { };
in
[
  # HIL Specific
  awscli2
  cloudflared
  git
  gnutar
  orb-hil
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
