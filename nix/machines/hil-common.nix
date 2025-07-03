# NixOS configuration common to all HILs. Combined with `nixos-common.nix`
{ config, pkgs, lib, hostname, ... }:
let
  username = "worldcoin";
  ghRunnerUser = "gh-runner-user";
in
{
  networking.hostName = "${hostname}";

  # Bootloader.
  boot.loader.systemd-boot.enable = true;
  boot.loader.efi.canTouchEfiVariables = true;

  # networking.wireless.enable = true;  # Enables wireless support via wpa_supplicant.

  # Configure network proxy if necessary
  # networking.proxy.default = "http://user:password@proxy:port/";
  # networking.proxy.noProxy = "127.0.0.1,localhost,internal.domain";

  # Enable networking
  networking.networkmanager.enable = true;

  # Set your time zone.
  time.timeZone = "America/New_York";

  # Select internationalisation properties.
  i18n.defaultLocale = "en_US.UTF-8";

  i18n.extraLocaleSettings = {
    LC_ADDRESS = "en_US.UTF-8";
    LC_IDENTIFICATION = "en_US.UTF-8";
    LC_MEASUREMENT = "en_US.UTF-8";
    LC_MONETARY = "en_US.UTF-8";
    LC_NAME = "en_US.UTF-8";
    LC_NUMERIC = "en_US.UTF-8";
    LC_PAPER = "en_US.UTF-8";
    LC_TELEPHONE = "en_US.UTF-8";
    LC_TIME = "en_US.UTF-8";
  };

  # Enable the X11 windowing system.
  services.xserver.enable = true;

  # Enable the KDE Plasma Desktop Environment.
  services.xserver.displayManager.sddm.enable = true;
  services.xserver.desktopManager.plasma5.enable = true;

  # Configure keymap in X11
  services.xserver = {
    layout = "us";
    xkbVariant = "";
  };

  # Enable CUPS to print documents.
  services.printing.enable = true;

  services.pipewire = {
    enable = true;
    pulse.enable = false; # Disable pipewire-pulse. IMO we don't need it.
    wireplumber = {
      enable = true;
      configPackages = [
      ];
    };
  };
  # redundant, here for clarity. This should be false when using sound servers
  hardware.alsa.enable = false;

  security.rtkit.enable = true;
  # Enable touchpad support (enabled default in most desktopManager).
  # services.xserver.libinput.enable = true;

  users.users.${ghRunnerUser} = {
    isNormalUser = true;
    description = "User for github actions runner";
    extraGroups = [ "wheel" "plugdev" "dialout" ];
  };
  #
  # Allow unfree packages
  nixpkgs.config.allowUnfree = true;

  # Some programs need SUID wrappers, can be configured further or are
  # started in user sessions.
  # programs.mtr.enable = true;
  # programs.gnupg.agent = {
  #   enable = true;
  #   enableSSHSupport = true;
  # };

  # List services that you want to enable:

  # Open ports in the firewall.
  # networking.firewall.allowedTCPPorts = [ ... ];
  # networking.firewall.allowedUDPPorts = [ ... ];
  # Or disable the firewall altogether.
  # networking.firewall.enable = false;

  services.teleport = {
    enable = true;
    # Currently, the internal cluster requires no newer than v12
    package = pkgs.nixpkgs-23_11.teleport_12;
  };

  services.github-runners = {
    "${hostname}" = {
      enable = true;
      name = "${hostname}";
      package = pkgs.unstable.github-runner;
      url = "https://github.com/worldcoin/orb-os";
      tokenFile = "/etc/worldcoin/secrets/gh-runner-token";
      extraLabels = [ "nixos" "flashing-hil" "${hostname}" ];
      replace = true;
      user = ghRunnerUser;
      serviceOverrides = {
        DynamicUser = lib.mkForce false;
        PrivateTmp = false;
        PrivateMounts = false;
        PrivateDevices = false;
        ProtectClock = false;
        ProtectControlGroups = false;
        ProtectHome = false;
        ProtectHostname = false;
        ProtectKernelLogs = false;
        ProtectKernelModules = false;
        ProtectKernelTunables = false;
        ProtectProc = "default";
        ProtectSystem = "";
        RestrictNamespaces = false;
        SystemCallFilter = lib.mkForce [ ];
      };
    };
  };
}
