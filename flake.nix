{
  description = "orb-software flake";
  inputs = {
    # Different versions of nixpkgs
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.05";
    nixpkgs-unstable.url = "github:NixOS/nixpkgs/nixos-unstable";
    nixpkgs-23_11.url = "github:NixOS/nixpkgs/nixos-23.11";
    # Provides eachDefaultSystem and other utility functions
    flake-utils.url = "github:numtide/flake-utils";
    # Replacement for rustup
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    pyproject-nix = {
      url = "github:pyproject-nix/pyproject.nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    uv2nix = {
      url = "github:pyproject-nix/uv2nix";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.pyproject-nix.follows = "pyproject-nix";
    };
    pyproject-build-systems = {
      url = "github:pyproject-nix/build-system-pkgs";
      inputs.pyproject-nix.follows = "pyproject-nix";
      inputs.uv2nix.follows = "uv2nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    # Replaces the need to have a git submodule.
    #
    # Pinned to an explicit rev (not a branch ref) so `nix flake update` never
    # needs the GitHub ref->rev API call (api.github.com). That call requires
    # a valid token for this private repo, and on hosts with a stale/wrong-scope
    # token it 404s (GitHub hides private repos from unauthorized requests
    # rather than 401ing), breaking `nix flake update` entirely. Bump the rev
    # by hand (with a valid token) when seek-thermal-sdk needs updating.
    seekSdk = {
      url = "github:worldcoin/seek-thermal-sdk/63471a7cdc2cd06e1a317ec31c26b9da8aa8f883";
      flake = false;
    };
  };

  outputs =
    inputs:
    let
      # Used for conveniently accessing nixpkgs on different platforms.
      # We instantiate this once here, and then use it in various other places.
      p = (import ./nix/packages/nixpkgs.nix { inherit inputs; });
      # Creates a `nix develop` shell for every host platform.
      devShells = (
        import nix/shells/flake-outputs.nix {
          inherit inputs;
          instantiatedPkgs = p;
        }
      );
      containers = (
        import nix/containers/flake-outputs.nix {
          inherit inputs;
          instantiatedPkgs = p;
        }
      );
      lib = inputs.nixpkgs.lib;
    in

    # This is like repeatedly doing a deep version of the  `//` operator to combine into one big attrset.
    lib.foldl' lib.recursiveUpdate { } [
      devShells
      containers
    ];
}
