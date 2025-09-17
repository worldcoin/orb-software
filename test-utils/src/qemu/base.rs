use super::img::QemuImg;

const NET_ENP0S3_SVC: &str = "
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
";

const RESOLVED_CONF: &str = "
[Resolve]
DNS=10.0.2.3 1.1.1.1
FallbackDNS=";

const SUDOERS: &str = "worldcoin ALL=(ALL) NOPASSWD:ALL";

const SSHD_CFG: &str = "
PasswordAuthentication yes
PermitEmptyPasswords yes
UsePAM yes
PubkeyAuthentication no
UseDNS no
GSSAPIAuthentication no
";

pub fn bullseye() -> QemuImg {
    QemuImg::from_base("debian-11-generic-amd64.qcow2")
        .write("/etc/systemd/system/net-enp0s3.service", NET_ENP0S3_SVC)
        .run("systemctl enable net-enp0s3.service")
        .run("systemctl disable systemd-resolved")
        .write("/etc/systemd/resolved.conf.d/99-qemu.conf", RESOLVED_CONF)
        .run("ln -sf /run/systemd/resolve/stub-resolv.conf /etc/resolv.conf")
        .run("systemctl enable systemd-resolved")
        .write("/etc/sudoers.d/worldcoin", SUDOERS)
        .write("/etc/ssh/sshd_config.d/99-local.conf", SSHD_CFG)
        .run("useradd -m -s /bin/bash -G sudo worldcoin")
        .run("passwd -d worldcoin")
        .pkgs(&["openssh-server", "iproute2", "network-manager"])
        .run("systemctl enable ssh")
        .run("ssh-keygen -A")
}
