# Packages for the HIL.
{ pkgs, ... }: with pkgs; [
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
  libiconv # see https://stackoverflow.com/a/69732679
  ninja
  zig
]
