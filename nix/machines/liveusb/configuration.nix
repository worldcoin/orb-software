{
  inputs,
  pkgs,
  modulesPath,
  lib,
  system,
  hostname,
  ...
}:
let
  username = "worldcoin";
in
{
  imports = [
    # "${modulesPath}/installer/cd-dvd/installation-cd-minimal.nix"
    ./hardware-configuration.nix
  ];

  nix = {
    package = pkgs.nix;
    channel.enable = false;
    nixPath = lib.mkForce [ "nixpkgs=flake:nixpkgs" ];
    settings = {
      "experimental-features" = [
        "nix-command"
        "flakes"
      ];
      "max-jobs" = "auto";
      trusted-users = [
        "root"
        "@admin"
        username
      ];
    };
    extraOptions = ''
      netrc-file = /run/nix-github-netrc
    '';
  };

  # Empty placeholder so nix.conf's netrc-file always resolves to something;
  # curl hard-fails (even on unrelated public fetches) if the file is missing
  # entirely, but is fine with it being empty. disko-install fills it in.
  systemd.tmpfiles.rules = [ "f /run/nix-github-netrc 0600 root root -" ];

  # crates.io blocks the default curl User-Agent used by fetchurl.
  environment.variables.NIX_CURL_FLAGS = "--user-agent orb-software-nix-fetcher";
  systemd.services.nix-daemon.environment.NIX_CURL_FLAGS = "--user-agent orb-software-nix-fetcher";

  # use the latest Linux kernel
  boot = {
    kernelPackages = pkgs.linuxPackages_latest;
    # BEGIN recommendations from disko:
    # https://github.com/nix-community/disko/blob/abc8baff/docs/quickstart.md
    loader.systemd-boot.enable = true;
    loader.efi.canTouchEfiVariables = true;
    # loader.grub.enable = true;
    # loader.grub.efiSupport = true;
    # loader.grub.efiInstallAsRemovable = true;
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

  # Resize rootfs to remaining disk space on boot
  systemd.repart = {
    enable = true;
    partitions = {
      # See https://www.freedesktop.org/software/systemd/man/latest/repart.d.html
      "10-root" = {
        Type = "root"; # match the existing root partition
        GrowFileSystem = "yes"; # use systemd-growfs too. Technically is redundant.
      };
    };
  };
  fileSystems."/".autoResize = true;

  # Define a user account. Don't forget to set a password with ‘pas.
  users.groups = {
    plugdev = { };
  };
  users.users.${username} = {
    isNormalUser = true;
    extraGroups = [
      "dialout" # serial access
      "networkmanager" # wifi control
      "plugdev" # usb access
      "wheel" # sudo powers
    ]; # Enable ‘sudo’ for the user.
    openssh.authorizedKeys.keys = import ../ssh-keys.nix;
  };
  users.mutableUsers = false;

  programs.zsh.enable = true;
  programs.nix-ld.enable = true;

  security.sudo.wheelNeedsPassword = false;
  services.getty = {
    autologinUser = "${username}";
    greetingLine = "Welcome to Worldcoin's custom NixOS Liveusb";
    helpLine = ''
      Follow instructions at https://worldcoin.github.io/orb-software/hil/nixos-setup.html
      To connect to internet, either plug in an ethernet cable, or connect to wifi
      with the following (be sure to use single quotes!):

      nmcli device wifi connect 'My Wifi SSID' password 'My Password'

      Sometimes you first have to run:

      nmcli connection delete 'My Wifi SSID'
    '';

  };

  # Enable the OpenSSH daemon.
  services.openssh = {
    enable = true;
    settings.PasswordAuthentication = false;
  };
  services.tailscale.enable = true; # since teleport won't be set up

  # USB stuff
  services.udev = {
    enable = true;
    extraRules = ''
      SUBSYSTEM=="usb", MODE="0660", GROUP="plugdev"
    '';
  };

  services.resolved = {
    enable = true;
    # set to "false" if giving you trouble
    dnsovertls = "opportunistic";
    # dnsovertls = "false";
  };

  environment.systemPackages =
    with pkgs;
    [
      curl
      git
      gptfdisk
      neovim # Do not forget to add an editor to edit configuration.n.
      parted
      usbutils
      vim
      wget
    ]
    ++ [
      # Wraps the real disko-install: prompts once for a GitHub token (needed
      # to fetch private flake inputs like seekSdk) and feeds it to Nix via
      # NIX_USER_CONF_FILES. The token only ever lives in /run (tmpfs), never
      # on persistent disk or in the image itself.
      (pkgs.writeShellApplication {
        name = "disko-install";
        text = ''
          conf=/run/nix-github-access.conf
          netrc=/run/nix-github-netrc
          if [ ! -f "$conf" ]; then
            read -rs -p "GitHub token for private flake inputs (Enter to skip): " token
            echo
            if [ -n "$token" ]; then
              install -m 600 /dev/null "$conf"
              printf 'access-tokens = github.com=%s\n' "$token" > "$conf"

              printf 'machine github.com\n  login x-access-token\n  password %s\n' "$token" > "$netrc"
            fi
          fi
          # Set explicitly (not just via environment.variables) since sudo
          # doesn't reliably inherit ambient/PAM-sourced environment here.
          exec env \
            NIX_USER_CONF_FILES="$conf" \
            NIX_CURL_FLAGS="--user-agent orb-software-nix-fetcher" \
            ${inputs.disko.packages.${system}.disko-install}/bin/disko-install "$@"
        '';
      })
      inputs.disko.packages.${system}.disko
    ];

  # Enable networking
  networking.networkmanager.enable = true;
  networking.wireless.enable = false;
  networking.hostName = hostname;

  # Set your time zone.
  time.timeZone = "America/New_York";
  # Select internationalisation properties.
  i18n.defaultLocale = "en_US.UTF-8";

  # The config was written with 24.05 in mind. Don't change it unless you have
  # reviewed the new settings options.
  system.stateVersion = lib.mkForce "24.05";
}
