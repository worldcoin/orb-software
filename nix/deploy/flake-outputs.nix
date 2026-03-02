# Deployment configuration for HIL machines using deploy-rs
{ inputs, p, ... }:
let
  inherit (inputs) deploy-rs nixpkgs;

  # Helper to create a deployment config for a HIL machine
  mkHilDeploy =
    { hostname }:
    {
      inherit hostname;
      profiles.system = {
        user = "root";
        sshUser = "worldcoin";
        path = deploy-rs.lib.x86_64-linux.activate.nixos inputs.self.nixosConfigurations.${hostname};
        sshOpts = [
          "-o"
          "StrictHostKeyChecking=no"
          "-o"
          "UserKnownHostsFile=/dev/null"
        ];
        magicRollback = true;
        autoRollback = true;
      };
    };

  # All HIL machines that we can deploy to
  hilMachines = [
    "worldcoin-hil-munich-2"
    "worldcoin-hil-munich-5"
    "worldcoin-hil-munich-9"
  ];

  # Convert list of machines to deploy-rs nodes
  nodes = nixpkgs.lib.listToAttrs (
    map (hostname: {
      name = hostname;
      value = mkHilDeploy { inherit hostname; };
    }) hilMachines
  );
in
{
  deploy = {
    inherit nodes;

    # This is highly advised, and will prevent many possible mistakes
    sshUser = "worldcoin";
    user = "root";

    # Enable magic rollback
    magicRollback = true;
    autoRollback = true;
  };

  # Add deploy-rs checks
  checks = builtins.mapAttrs (
    system: deployLib: deployLib.deployChecks inputs.self.deploy
  ) deploy-rs.lib;
}
