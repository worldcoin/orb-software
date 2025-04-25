# HIL NixOS Setup

Eventually, we will support making installation media from the NixOS config
directly, including setup scripts to fully automate this process. But *for now*,
one first needs to do a lot of manual bootstrapping.

### Installing NixOS to a liveusb

On the ASUS NUCs, they don't support MBR partitioned live usbs. But for some
inexplicable reason the official NixOS installer *only* exists as a MBR partitioned
disk. This means we need to build our own GPT/UEFI based NixOS live usb ;(

To work around this limitation of the official installer, we provide a liveusb
image that has NixOS on it, and is built using the
[nixos-generators][nixos-generators] tool. To build the .img file yourself,
run:
```bash
nix build .#liveusb # if you are natively on x86-64 linux
```
If you are *not* on x86-64 linux, you should either ssh into a x86 linux
machine (such as one of the existing HILs) and SCP the image off, *or* you
should use the [remote builders][remote build] feature of nix. Here is an example:

```bash
 # ensure that sshing as root works, and that your ssh keys don't require any passwords, etc
sudo ssh -T user@hostname
# actually do the build
nix build .#packages.x86_64-linux.liveusb --builders 'ssh://user@hostname x86_64-linux - - - kvm'
```

### Use the liveusb to install NixOS

#### Booting from the liveusb

This is the same as any other linux liveusb. Get into your boot menu using the
function keys at boot, and select the USB from the boot options. If it doesn't
show up, make sure you are using a GPT/UEFI based liveusb. You will likely need
to disable UEFI secure boot as well.

#### Setting up Partitions

You need three partitions:
- EFI (512MB, format as FAT32)
- Swap (Make it 32GB or size of ram, whichever is bigger. Format as linux-swap)
- Rootfs (Rest of disk, format as ext4)

TODO: Describe how to do this from `parted`. See also [here](https://nixos.wiki/wiki/NixOS_Installation_Guide#UEFI) and [here](https://github.com/SfikasTeo/NixOS?tab=readme-ov-file#configuring-partitions-and-filesystems)

After you finish this step, the rootfs partition should be mounted to to /mnt and the EFI boot partition to /mnt/boot.

#### Install NixOS from the liveusb

1. Make sure that the new partitions are mounted under `/mnt` and `/mnt/boot`.
2. Run `sudo nixos-generate-config --root /mnt`. This will create a new nixos
   config for the NUC.
3. Edit the NUC's NixOS config at `/mnt/etc/nixos/configuration.nix` to be the
   following: TODO: Make sure this is all that is needed, and just use this to
   generate an image instead of them typing it in.

    `/mnt/etc/nixos/configuration.nix`:
    ```nix
    { config, pkgs, lib, ... }:
    let
      username = "worldcoin";
      hostname = "my-hostname-here";
      hashedPassword = ""; # paste output of mkpasswd here
    in
    {
      networking.hostName = "${hostname}";
      
      environment.systemPackages = with pkgs; [
        curl
        git
        neovim
        parted
        usbutils
        vim
      ];

      # Enable the OpenSSH daemon.
      services.openssh = {
        enable = true;
        passwordAuthentication = false;
      };

      # Enable the X11 windowing system.
      services.xserver.enable = true;
      # Enable the KDE Plasma Desktop Environment.
      services.xserver.displayManager.sddm.enable = true;
      services.xserver.desktopManager.plasma5.enable = true;

      # Enable networking
      networking.networkmanager.enable = true;

      users.users."${username}" = {
        isNormalUser = true;
        description = "${username}";
        hashedPassword = hashedPassword;
        extraGroups = [
          "networkmanager"
          "wheel" # Gives sudo
          "plugdev"
          "dialout"
        ];
      };

      # use the latest Linux kernel
      boot = {
        kernelPackages = pkgs.linuxPackages_latest;
        # Needed for https://github.com/NixOS/nixpkgs/issues/58959
        supportedFilesystems = lib.mkForce [ "btrfs" "reiserfs" "vfat" "f2fs" "xfs" "ntfs" "cifs" ];
      };
    }
    ```
4. Make sure that your liveusb is connected to the internet. If its not, you
   can use `nmtui` to connect.
5. `cd /mnt && sudo nixos-install`
6. `sudo shutdown -h now`, and remove the liveusb.
7. Boot the freshly installed NixOS (you may need to select it from the boot menu).
8. Make sure that all of the following is true:
  - You can boot into it.
  - You have internet access (you can connect to wifi with `nmtui`).
  - You have sudo rights

#### Switch to the full NixOS config.

Now that NixOS is installed on the NUC, we need to upgrade it to the full blown
config that we use. Luckily nix makes this really easy.

1. Clone the [nix config][nix config].
2. Customize the config to add an entry for your new machine. Be sure you set the
   hostname to be the same as what the current hostname is. You can ask @thebutlah
   to do this for you or look at the existing config to figure it out. Eventually we
   will make this really easy. Be sure that you add a ssh key for your account so that
   you can still access it in the case that teleport doesn't work.
3. (only if creating a self-hosted runner) Create a
   `/etc/worldcoin/secrets/gh-runner-token` file and populate it with the
   `orb-os-self-hosted-runner` token from 1Password.
4. Clone the [nix config][nix config] to ~/nix.
5. Run `sudo nixos-rebuild --impure --flake ~/nix`
6. Install teleport. Ask in slack for how to do this, its a bit involved, since
   it requires manually editing the shell script, as well as requesting access.

## Setting up Teleport

1. Request teleport token for a HIL in slack. You will receive a bash one-liner.

**DO NOT RUN THE BASH, THIS IS AN EXAMPLE:**
```bash
sudo bash -c "$(curl -fsSL https://teleport-cluster.orb.internal-tools.worldcoin.dev/scripts/ffffffffffffffffffffffffffffffff/install-node.sh)"
```
The command you received on slack should look like something of the above.

Instead of running the command, delete everything except the `curl` command and then
redirect that to a file called `teleport-install.sh`, for example:
```bash
curl -fsSL https://teleport-cluster.orb.internal-tools.worldcoin.dev/scripts/ffffffffffffffffffffffffffffffff/install-node.sh > teleport-install.sh

```

Be sure that `teleport-install.sh` is put on the HIL, you can put it in the home directory
for now. Again, *DO NOT RUN THIS SCRIPT*.

2. Place the following content on the HIL at `/etc/teleport.yaml`:
```yaml
version: v3
teleport:
  nodename: SED_HOSTNAME
  data_dir: /var/lib/teleport
  join_params:
    token_name: SED_TOKEN
    method: token
  proxy_server: teleport-cluster.orb.internal-tools.worldcoin.dev:443
  log:
    output: stderr
    severity: INFO
    format:
      output: text
  ca_pin: sha256:e0974d24cee9f3494a7ca9d8496f5c67f3fc60ee4bff2f823d2bbdb2c0ea4a2c
  diag_addr: ""
auth_service:
  enabled: "no"
ssh_service:
  enabled: "yes"
  labels:
    hostname: SED_HOSTNAME
  commands:
proxy_service:
  enabled: "no"
  https_keypairs: []
  https_keypairs_reload_interval: 0s
  acme: {}
```

3. run the following from the same directory that `teleport-install.sh` is at on the
HIL:
```bash
TELEPORT_TOKEN="$(cat teleport-install.sh | grep -m1 -oP "^JOIN_TOKEN='\K[^']+")"
TELEPORT_HOSTNAME="$(hostname)"
sudo sed -i "s/SED_TOKEN/${TELEPORT_TOKEN}/" /etc/teleport.yaml
sudo sed -i "s/SED_HOSTNAME/${TELEPORT_HOSTNAME}/" /etc/teleport.yaml
````

This will edit the contents of `/etc/teleport.yaml` to replace the `SED_*` strings with
your hostname and the token.

You can `sudo cat /etc/teleport.yaml` and inspect the file to see the new contents.

4. Run 
```bash
sudo rm -rf /var/lib/teleport
sudo systemctl restart teleport.service && sudo journalctl -fu teleport.service
```

You will see log messages from teleport. Make sure it looks roughly like everything
is normal. Teleport should now be set up.


[nix config]: https://github.com/TheButlah/nix
[nixos-generators]: https://github.com/nix-community/nixos-generators
[remote build]: https://nix.dev/manual/nix/2.18/advanced-topics/distributed-builds
