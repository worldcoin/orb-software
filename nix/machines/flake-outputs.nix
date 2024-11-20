# This file defines any machines that we use nix to provision.
#
# It gets directly combined with the toplevel flake.nix.
{ inputs, p, ... }:
let
  inherit (inputs) nixpkgs home-manager nixos-generators;
in
let
  # Helper function for all worldcoin NixOS machines.
  nixosConfig = { system, hostname, homeManagerCfg }: nixpkgs.lib.nixosSystem rec {
    inherit system;
    specialArgs = {
      inherit inputs hostname; pkgs = p.${system};
      modulesPath = "${nixpkgs}/nixos/modules";
    };
    modules = [
      ./${hostname}/configuration.nix
      inputs.nixos-generators.nixosModules.all-formats
      # setup home-manager
      home-manager.nixosModules.home-manager
      {
        home-manager = {
          useGlobalPkgs = true;
          useUserPackages = true;
          # include the home-manager module
          users."worldcoin" = import homeManagerCfg;
          extraSpecialArgs = rec {
            pkgs = p.${system}.pkgs;
          };
        };
        # https://github.com/nix-community/home-manager/issues/4026
        # users.users.${username}.home = s.${system}.pkgs.lib.mkForce "/Users/${username}";
      }
    ];
  };
  # Helper function for all HILs. Further specializes `nixosConfig`.
  hilConfig = { hostname }: nixosConfig {
    system = "x86_64-linux";
    hostname = "${hostname}";
    homeManagerCfg = ./home-hil.nix;
  };
in
# Machine list is here, if you are adding a new machine, don't edit anything
  # above this line.
{
  nixosConfigurations."ryan-worldcoin-hil" = hilConfig {
    hostname = "ryan-worldcoin-hil";
  };
  nixosConfigurations."worldcoin-hil-munich-0" = hilConfig {
    hostname = "worldcoin-hil-munich-0";
  };
  nixosConfigurations."worldcoin-hil-munich-1" = hilConfig {
    hostname = "worldcoin-hil-munich-1";
  };

  nixosConfigurations."liveusb" = inputs.nixpkgs.lib.nixosSystem rec {
    system = "x86_64-linux";
    specialArgs = {
      pkgs = p.${system};
      hostname = "liveusb";
      modulesPath = "${inputs.nixpkgs}/nixos/modules";
    };
    modules = [
      ./liveusb2.nix
      # inputs.nixos-generators.nixosModules.all-formats
    ];
  };

  packages.x86_64-linux.liveusb = nixos-generators.nixosGenerate {
    system = "x86_64-linux";
    specialArgs = { hostname = "liveusb"; };
    modules = [
      {
        # Pin nixpkgs to the flake input, so that the packages installed
        # come from the flake inputs.nixpkgs.url.
        nix.registry.nixpkgs.flake = nixpkgs;
      }
      ./liveusb.nix
    ];
    format = "iso";

    # optional arguments:
    # explicit nixpkgs and lib:
    # pkgs = nixpkgs.legacyPackages.x86_64-linux;
    # lib = nixpkgs.legacyPackages.x86_64-linux.lib;
    # additional arguments to pass to modules:
    # specialArgs = { myExtraArg = "foobar"; };
  };
}
