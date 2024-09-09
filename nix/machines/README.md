# NixOS Machines

These files provide configuration for the various machines that run NixOS.

Each machine has its own directory with its hostname. Each of these contain the
`configuration.nix` and `hardware-configuration.nix` that is typically found
under `/etc/nixos/` on a NixOS machine.

These are then used by `flake-outputs.nix` and combined with the toplevel
`flake.nix`.
