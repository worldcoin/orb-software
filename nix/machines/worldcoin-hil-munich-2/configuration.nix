# Edit this configuration file to define what should be installed on
# your system.  Help is available in the configuration.nix(5) man page
# and in the NixOS manual (accessible by running 'nixos-help').

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

  environment.etc."worldcoin/orb.yaml" = {
    text = ''
      orb_id: bce8234c
      platform: diamond

      # Pin controller configuration for orb-hil
      # Type of pin controller to use (ftdi, relay)
      pin_ctrl_type: ftdi
      serial_num: BG02N9B6

      serial_path: "/dev/serial/by-id/usb-FTDI_FT232R_USB_UART_BG02N9B6-if00-port0"

      main_mcu_debugger_serial: "002800105553500F20393256"
      security_mcu_debugger_serial: "003100193137510339383538"
    '';
    mode = "0644";
  };
}
