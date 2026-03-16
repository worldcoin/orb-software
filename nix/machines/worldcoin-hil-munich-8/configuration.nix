# Edit this configuration file to define what should be installed on
# your system.  Help is available in the configuration.nix(5) man page
# and in the NixOS manual (accessible by running ‘nixos-help’).

{
  config,
  pkgs,
  lib,
  ...
}:
{
  imports = [
    # Include the results of the hardware scan.
    ./hardware-configuration.nix
    ../nixos-common.nix
    ../hil-common.nix
  ];

  worldcoin.orbPlatform = "diamond";

  environment.etc."worldcoin/orb.yaml" = {
    text = ''
      orb_id: 0aaab97e
      platform: ${config.worldcoin.orbPlatform}
      # Pin controller configuration for orb-hil
      # Type of pin controller to use (ftdi, relay)
      pin_ctrl_type: numato_relay
      serial_path: "/dev/serial/by-id/usb-FTDI_FT232R_USB_UART_BG010290-if00-port0"
      relay_bank: "/dev/ttyACM0"
      relay_power_channel: 5
      relay_recovery_channel: 6
    '';
    mode = "0644";
  };
}
