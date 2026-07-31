# NixOS configuration shared by Orb Mini HIL runners.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  qdl-rs = pkgs.callPackage ../packages/qdl-rs.nix { };
  flashingRigPython = pkgs.python312.withPackages (
    ps: with ps; [
      kivy
      pyudev
    ]
  );
  flashingRigPath = "%h/orb-mini-utils/flashing-rig";
  nativeLibraryPath = lib.makeLibraryPath [ pkgs.systemd ];
  flashingRigPathEnv = lib.makeBinPath [
    pkgs.android-tools
    qdl-rs
  ];
  runnerUsers = [
    "worldcoin"
    "gh-runner-user"
  ]
  ++ lib.optional config.worldcoin.jenkinsAgent.enable "jenkins-agent-user";
in
{
  config = lib.mkIf (config.worldcoin.orbPlatform == "mini") {
    services.displayManager.autoLogin = {
      enable = true;
      user = "worldcoin";
    };

    services.udev.packages = [ pkgs.android-udev-rules ];

    services.udev.extraRules = ''
      # Qualcomm EDL (Emergency Download) mode
      SUBSYSTEM=="usb", ATTR{idVendor}=="05c6", ATTR{idProduct}=="9008", MODE="0660", GROUP="plugdev", TAG+="uaccess"

      # Orb Mini normal boot / ADB mode
      SUBSYSTEM=="usb", ATTR{idVendor}=="05c6", ATTR{idProduct}=="90db", MODE="0660", GROUP="plugdev", TAG+="uaccess"

      # Qualcomm fastboot
      SUBSYSTEM=="usb", ATTR{idVendor}=="05c6", ATTR{idProduct}=="d00d", MODE="0660", GROUP="plugdev", TAG+="uaccess"

      # USB relay board serial interface
      SUBSYSTEM=="tty", KERNEL=="ttyACM*", MODE="0660", GROUP="dialout", TAG+="uaccess"
    '';

    environment.systemPackages = [
      flashingRigPython
      qdl-rs
      pkgs.android-tools
    ];

    worldcoin.extraPythonPackages = with pkgs.python312Packages; [
      boto3
      kivy
      pyudev
    ];

    users.users = lib.genAttrs runnerUsers (_: {
      extraGroups = [
        "plugdev"
        "dialout"
      ];
    });

    systemd.user.services.flashing-rig = {
      description = "QDL Flashing Rig";
      after = [ "graphical-session.target" ];
      wants = [ "graphical-session.target" ];
      wantedBy = [ "graphical-session.target" ];
      serviceConfig = {
        WorkingDirectory = flashingRigPath;
        ExecStart = "${flashingRigPython}/bin/python ${flashingRigPath}/main.py";
        Restart = "always";
        RestartSec = 5;
        Environment = [
          "DISPLAY=:0"
          "LD_LIBRARY_PATH=${nativeLibraryPath}"
          "PATH=/run/wrappers/bin:/run/current-system/sw/bin:${flashingRigPathEnv}"
        ];
      };
    };
  };
}
