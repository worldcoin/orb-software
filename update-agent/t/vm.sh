#!/usr/bin/env sh

set -x
set -e
supermin --prepare bash util-linux systemd -o /tmp/supermin.d

cp ../cargo
supermin --build /tmp/supermin.d -f ext2 -o /tmp/appliance.d

qemu-kvm -nodefaults -nographic \
         -kernel /tmp/appliance.d/kernel \
         -initrd /tmp/appliance.d/initrd \
         -hda /tmp/appliance.d/root \
         -serial stdio -append "console=ttyS0 root=/dev/sda"
