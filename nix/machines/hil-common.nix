# NixOS configuration common to all HILs. Combined with `nixos-common.nix`
{ config, pkgs, lib, hostname, ... }:
let
  ghRunnerUser = "gh-runner-user";
in
{
  networking.hostName = "${hostname}";

  # Bootloader.
  # use the latest Linux kernel
  boot = {
    # BEGIN recommendations from disko:
    # https://github.com/nix-community/disko/blob/abc8baff/docs/quickstart.md
    #loader.systemd-boot.enable = true;
    #loader.efi.canTouchEfiVariables = true;
    loader.grub.enable = true;
    loader.grub.efiSupport = true;
    loader.grub.efiInstallAsRemovable = true;
    # loader.grub.device is set by disko automatically
    # END disko

    # Docs: https://elixir.bootlin.com/linux/v6.12.1/source/Documentation/admin-guide/serial-console.rst
    # All consoles listed here will be usable and are automatically logged into.
    # last console device is the one that gets boot logs. So in this case, vga.
    kernelParams = [
      "console=ttyS0,115200"
      "console=tty1"
    ];
  };

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

  # Enable sound with pipewire.
  sound.enable = true;
  hardware.pulseaudio.enable = false;
  security.rtkit.enable = true;
  services.pipewire = {
    enable = true;
    alsa.enable = true;
    alsa.support32Bit = true;
    pulse.enable = true;
    # If you want to use JACK applications, uncomment this
    #jack.enable = true;

    # use the example session manager (no others are packaged yet so this is enabled by default,
    # no need to redefine it in your config for now)
    #media-session.enable = true;
  };

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
