# HIL NixOS Setup

Eventually, we will support fully automating the setup process. But *for now*,
one first needs to do some manual bootstrapping.


## Installing NixOS to a liveusb

On the ASUS NUCs, they don't support MBR partitioned live usbs. But for some
inexplicable reason the official NixOS installer *only* exists as a MBR partitioned
disk. This means we need to build our own GPT/UEFI based NixOS live usb ;(

To work around this limitation of the official installer, we provide a liveusb
image that has NixOS on it, via [disko][disko]. The easiest way to get this liveusb image
is from the CI artifacts, it is built by the [Nix CI][nix ci] job.

Once you download it, unzip it and `zstd --decompress` it, you will have a `nixos.raw`
file. Plug your flashdrive in, identity the *disk* (not partition) of the flashdrive
using either `sudo fdisk -l` on linux or `Disk Utility` on macos. For example,
`/dev/sda` on linux (not `/dev/sda1`) or `/dev/diskX` on macos (not `/dev/diskXsY`).
Run the following:

```bash
sudo cp nixos.raw /dev/<your-usb-disk>
```
This loads the liveusb onto the flashdrive.


## Use the liveusb to install NixOS

### Booting from the liveusb

This is the same as any other linux liveusb. Get into your boot menu using the
function keys at boot, and select the USB from the boot options. Note: on the NUC, it
can only boot GPT/UEFI based liveusbs, MBR ones won't show up in the boot options. This
is why we had to build our own liveusb in the previous section. You will likely need
to disable UEFI secure boot as well.

### One-time setup of liveusb

**Be sure you have booted into your liveusb!!** From this point forward in the
instructions, these commands should be run on the booted HIL computer, not on your
laptop.

The image we build in CI is smaller than the actual size of your usb stick. We need
to increase its size to be able to have enough space to download the things we need.

```bash
sudo parted /dev/sda resizepart 3 100% # Select "Fix" and "Yes" if it prompts you
sudo resize2fs /dev/sda3
```

### Configuring WIFI

You can use `nmcli`.
```
nmcli device wifi connect "Your SSID Here" password "your password here"
```

### Performing installation

Assuming your intended hostname is `worldcoin-hil-sf-0`, run:

```bash
git clone https://github.com/worldcoin/orb-software.git --branch <your branch> ~/orb-software
sudo disko-install --flake ~/orb-software#worldcoin-hil-sf-0 --disk main /dev/nvme0n1
```

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

You will also need to make sure your machine's hostname matches the regex in our
terraform config [here][tf hil].


[nix config]: https://github.com/TheButlah/nix
[remote build]: https://nix.dev/manual/nix/2.18/advanced-topics/distributed-builds
[disko]: https://github.com/nix-community/disko
[tf hil]: https://github.com/worldcoin/infrastructure/blob/345bc7db0c47e369ce6529d0febed9535a0970f7/teleport/orb/orb-sw-dev-tools-teleport.tf
[nix ci]: https://github.com/worldcoin/orb-software/actions/workflows/nix-ci.yaml
