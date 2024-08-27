# Hardware In Loop

Developing for the orb generally requires access to an orb. To make life easy,
as well as to enable automated tests, we use an x86 linux machine as a host, which
attaches to an orb via a number of hardware peripherals. We created the `orb-hil`
cli tool to leverage this known hardware setup to perform a number of common,
useful actions.

## Getting an x86 Linux Machine

Technically, any x86 linux machine will do. However, we recommend using an
ASUS/Intel NUC due to its compact form factor.

The Linux installation needs:
* Access to the various attached usb devices without sudo, i.e. udev rules configured
* Access to serial without sudo
* Various packages (awscli2, usbutils, etc)
* Teleport
* Github self-hosted runner (if using this in CI).

To make this setup easy, we have a [nix config][nix config] that sets all of
this up. BUT you could use regular ubuntu, or some other linux distro instead.

For the NixOS approach, see the [nixos setup][nixos setup]. If you use NixOS, we
can manage all the machines in one git repo, so this is the prefrred option, even
though the initial setup is a bit more hassle (for now).

[nixos setup]: ./nixos-setup.md
[nix config]: https://github.com/TheButlah/nix
