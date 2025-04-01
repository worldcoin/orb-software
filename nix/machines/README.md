# NixOS Machines

These files provide configuration for the various machines that run NixOS.

Each machine has its own directory with its hostname. Each of these contain the
`configuration.nix` and `hardware-configuration.nix` that is typically found
under `/etc/nixos/` on a NixOS machine.

These are then used by `flake-outputs.nix` and combined with the toplevel
`flake.nix`.

## Building liveusb

The liveusb artifact is uploaded to CI. If you want to build it locally, you need an
x86 linux machine (or a remote or [virtualized][linux-builder] x86 nix builder).

```bash
nix build .#nixosConfigurations.liveusb.config.system.build.diskoImagesScript
./result --build-memory 2048
```

This will produce a liveusb.raw that you can simply `dd` to a flash drive (`/dev/sda`
for example).

[linux-builder]: https://daiderd.com/nix-darwin/manual/index.html#opt-nix.linux-builder.enable
