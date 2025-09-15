use std::path::PathBuf;

/*
# img to download
https://cloud-images.ubuntu.com/minimal/releases/jammy/release/ubuntu-22.04-minimal-cloudimg-amd64.img
https://cloud.debian.org/images/cloud/bullseye/latest/debian-11-generic-amd64.qcow2
https://cloud.debian.org/images/cloud/bookworm/latest/debian-12-generic-amd64.qcow2
https://cloud.debian.org/images/cloud/trixie/latest/debian-13-generic-amd64.qcow2

qemu-img create -f qcow2 -F qcow2 -b ./ubuntu-22.04-minimal-cloudimg-amd64.img prebake.qcow2

# download image
https://cloud.debian.org/images/cloud/bullseye/latest/debian-11-generic-amd64.qcow2

# create overlay
qemu-img create -f qcow2 -F qcow2 -b debian-11-generic-amd64.qcow2 prebake.qcow2

# create tmp write overlay
qemu-img create -f qcow2 -F qcow2 -b prebake.qcow2 run.qcow2

# customize
virt-customize -a db13.qcow2 \
  --run-command "cat > /etc/systemd/system/net-enp0s3.service <<'EOF'
[Unit]
Description=Bring up enp0s3 static
Before=network.target

[Service]
Type=oneshot
ExecStart=/usr/sbin/ip link set enp0s3 up
ExecStart=/usr/sbin/ip addr add 10.0.2.15/24 dev enp0s3
ExecStart=/usr/sbin/ip route add default via 10.0.2.2
RemainAfterExit=yes

[Install]
WantedBy=multi-user.target
EOF" \
  --run-command 'systemctl enable net-enp0s3.service || true' \
  --run-command 'systemctl disable systemd-resolved || true' \
  --run-command 'mkdir -p /etc/systemd/resolved.conf.d' \
  --write /etc/systemd/resolved.conf.d/99-qemu.conf:$'[Resolve]\nDNS=10.0.2.3 1.1.1.1\nFallbackDNS=\n' \
  --run-command 'ln -sf /run/systemd/resolve/stub-resolv.conf /etc/resolv.conf' \
  --run-command 'systemctl enable systemd-resolved' \
  --write '/etc/sudoers.d/worldcoin:worldcoin ALL=(ALL) NOPASSWD:ALL' \
  --write /etc/ssh/sshd_config.d/99-local.conf:$'PasswordAuthentication yes\nPermitEmptyPasswords yes\nUsePAM yes\nPubkeyAuthentication no\nUseDNS no\nGSSAPIAuthentication no\n' \
  --run-command 'useradd -m -s /bin/bash -G sudo worldcoin || true' \
  --run-command 'passwd -d worldcoin' \
  --run-command "set -eux; \
    mkdir -p /var/lib/apt/lists /var/cache/apt/archives /var/log; \
    mount -t tmpfs tmpfs /var/lib/apt/lists; \
    mount -t tmpfs tmpfs /var/cache/apt/archives; \
    mount -t tmpfs tmpfs /var/log; \
    apt-get update; \
    DEBIAN_FRONTEND=noninteractive apt-get -y install --no-install-recommends openssh-server iproute2; \
    systemctl enable ssh; \
    ssh-keygen -A; \
    umount /var/lib/apt/lists || true; \
    umount /var/cache/apt/archives || true; \
    umount /var/log || true"

# start vm
qemu-system-x86_64 \
  -machine q35,accel=kvm -cpu host -m 2048 -daemonize -display none -pidfile vm.pid \
  -monitor unix:vm.hmp,server,nowait -qmp unix:vm.qmp,server,nowait \
  -drive file=db13.qcow2,if=virtio,format=qcow2 \
  -object rng-random,filename=/dev/urandom,id=rng0 \
  -device virtio-rng-pci,rng=rng0 \
  -netdev user,id=n0,hostfwd=tcp:127.0.0.1:2222-:22 \
  -device e1000,netdev=n0

socat - UNIX-CONNECT:vm.hmp <<< "info usernet"

# shutdown
echo system_powerdown | socat - UNIX-CONNECT:vm.hmp

sudo modprobe mac80211_hwsim radios=1
sudo modprobe wwan_hwsim

*/

pub struct Qemu {
    working_dir: PathBuf,
    port: u16,
}

impl Qemu {
    const DEFAULT_PKGS: &[&str] = &["openssh-server"];
    const DEFAULT_CMDS: &[&str] = &["systemctl enable ssh"];

    /// Builds an Ubuntu Qemu instance using cloud-init. Caches ubuntu image, disk image, and
    /// cloud-init image.
    pub async fn build(
        working_dir: PathBuf,
        cache_dir: PathBuf,
        packages: &[&str],
        setup: &[&str],
        memory: usize,
    ) -> Self {
        let packages = Self::DEFAULT_PKGS.iter().chain(packages);
        let cmds = Self::DEFAULT_CMDS.iter().chain(setup);

        // todo: improve
        let sig: String = packages.chain(cmds).copied().collect();

        let port = 8080;

        Self { working_dir, port }
    }
}
