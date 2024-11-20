{ config, modulesPath, pkgs, ... }:
let
  username = "worldcoin";
in
{
  imports = [
    "${modulesPath}/installer/cd-dvd/installation-cd-minimal.nix"

    # Provide an initial copy of the NixOS channel so that the user
    # doesn't need to run "nix-channel --update" first.
    # "${modulesPath}/installer/cd-dvd/channel.nix}"
  ];

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

  # use the latest Linux kernel
  boot = {
    kernelPackages = pkgs.linuxPackages_latest;
  };


  # users.groups = {
  #   plugdev = { };
  # };
  # users.users."${username}" = {
  #   isNormalUser = true;
  #   description = "${username}";
  #   extraGroups = [
  #     "dialout" # serial access
  #     "networkmanager" # wifi control
  #     "plugdev" # usb access
  #     "wheel" # sudo powers
  #   ];
  #   openssh.authorizedKeys.keys = [
  #     "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBLmHbuCMFpOKYvzMOpTOF+iMX9rrY6Y0naarcbWUV8G ryan@ryan-laptop.local"
  #     "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIEoVo3BKge5tQuYpDuWKJaypdpfUuw4cq3/BYRFNovtj ryan.butler@Ryan-Butler.local"
  #     "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIIOhklnZHdjM0VD82Z1naZaoeM3Lr9dbrsM0r+J9sHqN alex@hq-small"
  #   ];
  #   # Check 1Password for access.
  #   # Note: This hash is publicly visible. Don't expose the user of the liveusb
  #   # to password based third party acces, (i.e. don't enable password based
  #   # ssh).
  #   # Also, do not reuse this pasword for anything that is actually security
  #   # sensitive.
  #   password = "publiclyknownpassword";
  #
  #   shell = pkgs.zsh;
  #   packages = with pkgs; [ ];
  # };
  # users.mutableUsers = false;
  security.sudo.wheelNeedsPassword = false;

  programs.zsh.enable = true;
  programs.nix-ld.enable = true;

  # Enable the OpenSSH daemon.
  services.openssh = {
    enable = true;
    passwordAuthentication = false;
  };
  # Automatically log in at the virtual consoles.
  services.getty.autologinUser = username;

  # USB stuff
  services.udev = {
    enable = true;
    extraRules = ''
      SUBSYSTEM=="usb", MODE="0660", GROUP="plugdev"
    '';
  };

  environment.systemPackages = with pkgs; [
    awscli2
    curl
    gh
    neovim
    parted
    picocom
    ripgrep
    usbutils
    vim
    zellij
  ];

}
