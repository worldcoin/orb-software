# Jenkins inbound (WebSocket/JNLP) agent, mirroring how `services.github-runners`
# is wired in hil-common.nix. This lets a HIL machine act as a Jenkins agent in
# addition to (or instead of) a GitHub Actions runner.
#
# The agent dials *out* to the Jenkins controller over WebSocket, so it works
# behind office NAT with no inbound ports open (same model as our GH runners).
#
# Disabled by default. Enable per-machine with:
#   worldcoin.jenkinsAgent = {
#     enable = true;
#     url = "https://<your-jenkins-controller>";
#   };
{
  config,
  pkgs,
  lib,
  hostname,
  ...
}:
let
  cfg = config.worldcoin.jenkinsAgent;
  agentUser = "jenkins-agent-user";
in
{
  options.worldcoin.jenkinsAgent = {
    enable = lib.mkEnableOption "Jenkins inbound (WebSocket/JNLP) agent";

    url = lib.mkOption {
      type = lib.types.str;
      example = "https://jenkins.internal.worldcoin.org";
      description = "Base URL of the Jenkins controller.";
    };

    nodeName = lib.mkOption {
      type = lib.types.str;
      default = hostname;
      description = ''
        Name of the node as registered in Jenkins under
        Manage Jenkins -> Nodes -> New Node. Defaults to the machine hostname.
        The node must be created as a "Permanent Agent" with launch method
        "Launch agent by connecting it to the controller".
      '';
    };

    secretFile = lib.mkOption {
      type = lib.types.path;
      default = "/etc/worldcoin/secrets/jenkins-agent-secret";
      description = ''
        File containing the connection secret shown on the Jenkins node page.
        Provisioned out-of-band (like /etc/worldcoin/secrets/gh-runner-token),
        readable only by root; loaded via systemd LoadCredential.
      '';
    };

    workDir = lib.mkOption {
      type = lib.types.str;
      default = "/var/lib/jenkins-agent";
      description = "Remote root / work directory for the agent.";
    };

    javaPackage = lib.mkOption {
      type = lib.types.package;
      default = pkgs.jdk21_headless;
      description = "JDK used to run the agent. Must satisfy the controller's Java requirement.";
    };

    extraGroups = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [
        "plugdev"
        "dialout"
      ];
      description = "Extra groups for the agent user (HIL hardware access: USB relays, serial).";
    };
  };

  config = lib.mkIf cfg.enable {
    users.users.${agentUser} = {
      isNormalUser = true;
      description = "User for Jenkins inbound agent";
      home = cfg.workDir;
      createHome = true;
      extraGroups = [ "wheel" ] ++ cfg.extraGroups;
    };
    users.groups.${agentUser} = {
      members = [ agentUser ];
    };

    systemd.tmpfiles.rules = [
      "d ${cfg.workDir} 0755 ${agentUser} ${agentUser} - -"
    ];

    systemd.services.jenkins-agent = {
      description = "Jenkins inbound agent (${cfg.nodeName})";
      after = [ "network-online.target" ];
      wants = [ "network-online.target" ];
      wantedBy = [ "multi-user.target" ];
      # Tools the build/test steps typically need on PATH, plus the JDK.
      path = [
        cfg.javaPackage
        pkgs.curl
        pkgs.git
        pkgs.bash
      ];
      serviceConfig = {
        User = agentUser;
        WorkingDirectory = cfg.workDir;
        Restart = "always";
        RestartSec = 10;
        # Expose the secret at $CREDENTIALS_DIRECTORY/secret without leaking it
        # into the process table or the Nix store.
        LoadCredential = "secret:${toString cfg.secretFile}";
      };
      script = ''
        set -euo pipefail

        # Fetch the agent.jar that matches the controller's version so the
        # protocol never drifts out of sync after a controller upgrade.
        curl -fsSL -o "${cfg.workDir}/agent.jar" "${cfg.url}/jnlpJars/agent.jar"

        exec java -jar "${cfg.workDir}/agent.jar" \
          -url "${cfg.url}" \
          -name "${cfg.nodeName}" \
          -secret "@$CREDENTIALS_DIRECTORY/secret" \
          -webSocket \
          -workDir "${cfg.workDir}"
      '';
    };
  };
}
