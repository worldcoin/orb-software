# NixOS configuration common to all HILs. Combined with `nixos-common.nix`
{ pkgs, lib, system, ... }:
let
  bashCmd = "${pkgs.nixpkgs-23_11.bash}/bin/bash";
  # things needed to be found by linker
  ldLibPath = lib.makeLibraryPath (with pkgs.nixpkgs-23_11; [
    alsaLib
    glib
    glibc
    lzma
    squashfs-tools-ng
  ]);
in
pkgs.dockerTools.buildLayeredImage {
  name = "nix-cargo-runner";
  tag = "latest";
  # things needed at runtime
  contents = with pkgs.nixpkgs-23_11; [
    alsaLib
    bash
    coreutils
    file
    gdb
    glib
    glibc
    glibc.bin
    gst_all_1.gst-plugins-base
    gst_all_1.gstreamer
    libsodium
    lzma
    nix-ld
    openssl
    squashfs-tools-ng
    squashfsTools
    udev
    which
  ];
  # See https://github.com/moby/docker-image-spec/blob/f1d00ebd/spec.md#image-json-description
  config = {
    Cmd = bashCmd;
    Env = [
      "LD_LIBRARY_PATH=${ldLibPath}"
    ];
    Volumes = {
      "/tmp" = { };
    };
  };
}
