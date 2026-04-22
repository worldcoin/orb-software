# This file defines any machines that we use nix to provision.
#
# It gets directly combined with the toplevel flake.nix.
{ inputs, p, ... }:
let
  inherit (inputs)
    nixpkgs
    home-manager
    nixos-generators
    disko
    ;
in
let
  # Helper function for all worldcoin NixOS machines.
  nixosConfig =
    {
      system,
      hostname,
      homeManagerCfg,
      diskoConfig,
      extraModules ? [ ],
    }:
    nixpkgs.lib.nixosSystem rec {
      specialArgs = {
        inherit inputs hostname system;
        modulesPath = "${nixpkgs}/nixos/modules";
      };
      modules = [
        # avoid errors due to the externally instantiated pkgs
        nixpkgs.nixosModules.readOnlyPkgs
        {
          nixpkgs = {
            pkgs = p.${system};
            flake = {
              setFlakeRegistry = true;
              setNixPath = true;
            };
          };
        }

        ./${hostname}/configuration.nix
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
        # setup disko for disk partitioning
        disko.nixosModules.disko
        diskoConfig
      ]
      ++ extraModules;
    };
  # Helper function for all HILs. Further specializes `nixosConfig`.
  hilConfig =
    { hostname, timezone }:
    nixosConfig {
      system = "x86_64-linux";
      hostname = "${hostname}";
      homeManagerCfg = ./home-hil.nix;
      diskoConfig = ./disko-bios-uefi-hil.nix;
      extraModules = [ { time.timeZone = timezone; } ];
    };
  # Machine list is here, if you are adding a new machine, don't edit anything
  # above this line.
in
{
  nixosConfigurations."ryan-worldcoin-hil" = hilConfig {
    hostname = "ryan-worldcoin-hil";
    timezone = "America/New_York";
  };
  nixosConfigurations."worldcoin-hil-jabil-0" = hilConfig {
    hostname = "worldcoin-hil-jabil-0";
    timezone = "Europe/Berlin";
  };
  nixosConfigurations."worldcoin-hil-munich-0" = hilConfig {
    hostname = "worldcoin-hil-munich-0";
    timezone = "Europe/Berlin";
  };
  nixosConfigurations."worldcoin-hil-munich-1" = hilConfig {
    hostname = "worldcoin-hil-munich-1";
    timezone = "Europe/Berlin";
  };
  nixosConfigurations."worldcoin-hil-munich-2" = hilConfig {
    hostname = "worldcoin-hil-munich-2";
    timezone = "Europe/Berlin";
  };
  nixosConfigurations."worldcoin-hil-munich-3" = hilConfig {
    hostname = "worldcoin-hil-munich-3";
    timezone = "Europe/Berlin";
  };
  nixosConfigurations."worldcoin-hil-munich-4" = hilConfig {
    hostname = "worldcoin-hil-munich-4";
    timezone = "Europe/Berlin";
  };
  nixosConfigurations."worldcoin-hil-munich-5" = hilConfig {
    hostname = "worldcoin-hil-munich-5";
    timezone = "Europe/Berlin";
  };
  nixosConfigurations."worldcoin-hil-munich-6" = hilConfig {
    hostname = "worldcoin-hil-munich-6";
    timezone = "Europe/Berlin";
  };
  nixosConfigurations."worldcoin-hil-munich-7" = hilConfig {
    hostname = "worldcoin-hil-munich-7";
    timezone = "Europe/Berlin";
  };
  nixosConfigurations."worldcoin-hil-munich-8" = hilConfig {
    hostname = "worldcoin-hil-munich-8";
    timezone = "Europe/Berlin";
  };
  nixosConfigurations."worldcoin-hil-munich-9" = hilConfig {
    hostname = "worldcoin-hil-munich-9";
    timezone = "Europe/Berlin";
  };
  nixosConfigurations."worldcoin-hil-munich-10" = hilConfig {
    hostname = "worldcoin-hil-munich-10";
    timezone = "Europe/Berlin";
  };
  nixosConfigurations."worldcoin-hil-munich-11" = hilConfig {
    hostname = "worldcoin-hil-munich-11";
    timezone = "Europe/Berlin";
  };
  nixosConfigurations."worldcoin-hil-sf-0" = hilConfig {
    hostname = "worldcoin-hil-sf-0";
    timezone = "America/Los_Angeles";
  };
  nixosConfigurations."liveusb" = nixosConfig {
    system = "x86_64-linux";
    hostname = "liveusb";
    homeManagerCfg = ./home-liveusb.nix;
    diskoConfig = ./disko-bios-uefi-liveusb.nix;
  };
}
