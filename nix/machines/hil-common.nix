# NixOS configuration common to all HILs. Combined with `nixos-common.nix`
{
  config,
  pkgs,
  lib,
  hostname,
  ...
}:
let
  username = "worldcoin";
  ghRunnerUser = "gh-runner-user";
  mkConnection = (
    number:
    let
      n = builtins.toString number;
    in
    {
      "Orb RCM Ethernet ${n}" = {
        connection = {
          autoconnect-priority = "-999";
          id = "Orb RCM Ethernet ${n}";
          interface-name = "orbeth${n}";
          type = "ethernet";
        };
        ethernet = { };
        ipv4 = {
          method = "shared"; # sets up DHCP server and shares internet
        };
        ipv6 = {
          addr-gen-mode = "default";
          method = "shared"; # sets up DHCP server and shares internet
        };
        proxy = { };
      };
    }
  );
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
  networking.networkmanager.ensureProfiles.profiles = lib.attrsets.mergeAttrsList [
    (mkConnection 0)
    (mkConnection 1)
    (mkConnection 2)
    (mkConnection 3)
  ];
  # Give the jetson USB ethernet a known name
  services.udev.extraRules = ''
    ACTION=="add", \
    SUBSYSTEM=="net", \
    SUBSYSTEMS=="usb", \
    ATTRS{idVendor}=="0955", \
    ATTRS{idProduct}=="7035", \
    NAME="orbeth%n"

    # Allow plugdev group to access USB relay hidraw devices
    KERNEL=="hidraw*", SUBSYSTEM=="hidraw", MODE="0664", GROUP="plugdev"
  '';

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
  services.displayManager.sddm.enable = true;
  services.desktopManager.plasma6.enable = true;

  # Configure keymap in X11
  services.xserver.xkb = {
    layout = "us";
    variant = "";
  };

  # Enable CUPS to print documents.
  services.printing.enable = true;

  services.pipewire = {
    enable = true;
    pulse.enable = false; # Disable pipewire-pulse. IMO we don't need it.
    wireplumber = {
      enable = true;
      configPackages = [ ];
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
    extraGroups = [
      "wheel"
      "plugdev"
      "dialout"
    ];
  };
  users.groups = {
    "${ghRunnerUser}" = {
      members = [ ghRunnerUser ];
    };
  };

  # Some programs need SUID wrappers, can be configured further or are
  # started in user sessions.
  # programs.mtr.enable = true;
  # programs.gnupg.agent = {
  #   enable = true;
  #   enableSSHSupport = true;
  # };

  # List services that you want to enable:

  # Open ports in the firewall.
  networking.firewall.allowedTCPPorts = [
    # all of these are nfs related: https://nixos.wiki/wiki/NFS#Firewall
    111
    2049
    4000
    4001
    4002
    20048
  ];
  networking.firewall.allowedUDPPorts = [
    # all of these are nfs related: https://nixos.wiki/wiki/NFS#Firewall
    67
    111
    2049
    4000
    4001
    4002
    20048
  ];
  # Or disable the firewall altogether.
  # networking.firewall.enable = false;

  services.nfs = {
    server = {
      enable = true;
      exports = ''
        /srv 10.42.0.0/24(rw,fsid=0,no_subtree_check,no_root_squash,crossmnt) # orbeth0 subnet
      '';
      # fixed rpc.statd port; for firewall
      lockdPort = 4001;
      mountdPort = 4002;
      statdPort = 4000;
      extraNfsdConfig = "";
    };
  };

  services.teleport = {
    enable = true;
    package = pkgs.teleport_17;
  };

  # VPN related services
  services.cloudflare-warp.enable = true;
  services.mullvad-vpn.enable = true;
  services.tailscale.enable = true;

  systemd.services."github-runner-${hostname}" = {
    serviceConfig = {
      InaccessiblePaths = lib.mkForce [ ];
    };
  };
  services.github-runners = {
    "${hostname}" = {
      enable = true;
      name = "${hostname}";
      package = pkgs.unstable.github-runner;
      url = "https://github.com/worldcoin/orb-os";
      tokenFile = "/etc/worldcoin/secrets/gh-runner-token";
      extraLabels = [
        "nixos"
        "flashing-hil"
        "${hostname}"
      ];
      replace = true;
      user = ghRunnerUser;

      serviceOverrides = {
        Environment = ''"PATH=/run/wrappers/bin:/run/current-system/sw/bin"''; # fixes missing sudo

        # Undo NixOS sandboxing
        CapabilityBoundingSet = [
          "CAP_CHOWN"
          "CAP_DAC_OVERRIDE"
          "CAP_DAC_READ_SEARCH"
          "CAP_FOWNER"
          "CAP_FSETID"
          "CAP_KILL"
          "CAP_SETGID"
          "CAP_SETUID"
          "CAP_SETPCAP"
          "CAP_LINUX_IMMUTABLE"
          "CAP_NET_BIND_SERVICE"
          "CAP_NET_BROADCAST"
          "CAP_NET_ADMIN"
          "CAP_NET_RAW"
          "CAP_IPC_LOCK"
          "CAP_IPC_OWNER"
          "CAP_SYS_MODULE"
          "CAP_SYS_RAWIO"
          "CAP_SYS_CHROOT"
          "CAP_SYS_PTRACE"
          "CAP_SYS_PACCT"
          "CAP_SYS_ADMIN"
          "CAP_SYS_BOOT"
          "CAP_SYS_NICE"
          "CAP_SYS_RESOURCE"
          "CAP_SYS_TIME"
          "CAP_SYS_TTY_CONFIG"
          "CAP_MKNOD"
          "CAP_LEASE"
          "CAP_AUDIT_WRITE"
          "CAP_AUDIT_CONTROL"
          "CAP_SETFCAP"
          "CAP_MAC_OVERRIDE"
          "CAP_MAC_ADMIN"
          "CAP_SYSLOG"
          "CAP_WAKE_ALARM"
          "CAP_BLOCK_SUSPEND"
          "CAP_AUDIT_READ"
          "CAP_PERFMON"
          "CAP_BPF"
          "CAP_CHECKPOINT_RESTORE"
        ];
        DynamicUser = lib.mkForce false;
        MemoryDenyWriteExecute = false;
        NoNewPrivileges = false;
        PrivateDevices = false;
        PrivateMounts = false;
        PrivateNetwork = false;
        PrivateTmp = false;
        PrivateUsers = false;
        ProtectClock = false;
        ProtectControlGroups = false;
        ProtectHome = false;
        ProtectHostname = false;
        ProtectKernelLogs = false;
        ProtectKernelModules = false;
        ProtectKernelTunables = false;
        ProtectProc = "default";
        ProtectSystem = "no";
        RemoveIPC = false;
        RestrictAddressFamilies = lib.mkForce [ ];
        RestrictNamespaces = false;
        RestrictRealtime = false;
        RestrictSUIDSGID = false;
        SystemCallFilter = lib.mkForce [ ];
      };
    };
  };
}
