{ config, pkgs, lib, hostname, ... }:
let
  username = "worldcoin";
in
{
  # imports = [
  #   ./nixos-common.nix
  # ];

  hardware.enableRedistributableFirmware = true;

  nix = {
    package = pkgs.nix;
    settings = {
      "experimental-features" = [ "nix-command" "flakes" "repl-flake" ];
      "max-jobs" = "auto";
      trusted-users = [
        "root"
        "@wheel"
        username
      ];
    };
  };
  nixpkgs.config.allowUnfree = true;

  users.groups = {
    plugdev = { };
  };
  users.users."${username}" = {
    isNormalUser = true;
    description = "${username}";
    extraGroups = [
      "dialout" # serial access
      "networkmanager" # wifi control
      "plugdev" # usb access
      "wheel" # sudo powers
    ];
    # For now, we only hard-code @thebutlah's keys. This allows remote access in case
    # teleport isn't working or is misconfigured.
    openssh.authorizedKeys.keys = [
      "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBLmHbuCMFpOKYvzMOpTOF+iMX9rrY6Y0naarcbWUV8G ryan@ryan-laptop.local"
      "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIEoVo3BKge5tQuYpDuWKJaypdpfUuw4cq3/BYRFNovtj ryan.butler@Ryan-Butler.local"
      "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIIOhklnZHdjM0VD82Z1naZaoeM3Lr9dbrsM0r+J9sHqN alex@hq-small"
    ];

    shell = pkgs.zsh;
    packages = with pkgs; [
      firefox
      # (python3.withPackages pythonShell)
    ];
  };
  users.mutableUsers = false;
  security.sudo.wheelNeedsPassword = false;

  programs.zsh.enable = true;
  programs.nix-ld.enable = true;

  environment.systemPackages = with pkgs; [
    awscli2 # todo: remove this when hil can be consumed via flake
    gh
    git
    neovim
    parted
    usbutils
    vim
    # (python3.withPackages pythonShell)
  ];

  # Enable the OpenSSH daemon.
  services.openssh = {
    enable = true;
    passwordAuthentication = false;
  };

  # USB stuff
  services.udev = {
    enable = true;
    extraRules = ''
      SUBSYSTEM=="usb", MODE="0660", GROUP="plugdev"
    '';
  };

  # services.resolved = {
  #   enable = true;
  #   # set to "false" if giving you trouble
  #   dnsovertls = "opportunistic";
  # };

  # use the latest Linux kernel
  boot = {
    # Use the GRUB 2 boot loader.
    loader.systemd-boot.enable = true;
    loader.efi.canTouchEfiVariables = true;

    kernelPackages = pkgs.linuxPackages_latest;
    # kernel.sysctl = {
    #   # Needed to run buildFHSEnv in github runner
    #   "kernel.unprivileged_userns_clone" = 1;
    # };
    # Needed for https://github.com/NixOS/nixpkgs/issues/58959
    supportedFilesystems = lib.mkForce [ "btrfs" "reiserfs" "vfat" "f2fs" "xfs" "ntfs" "cifs" ];
  };

  # Enable networking
  networking.networkmanager.enable = true;
  networking.wireless.enable = false;
  networking.hostName = hostname;

  # This value determines the NixOS release from which the default
  # settings for stateful data, like file locations and database versions
  # on your system were taken. It‘s perfectly fine and recommended to leave
  # this value at the release version of the first install of this system.
  # Before changing this value read the documentation for this option
  # (e.g. man configuration.nix or on https://nixos.org/nixos/options.html).
  system.stateVersion = "23.11";
}
