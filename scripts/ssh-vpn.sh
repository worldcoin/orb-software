#!/usr/bin/env bash

# A script to configure a `tun0` network interface. This can be used with `ssh -w`.
#
# DISCLAIMER: openssh does not exist on prod orbs. So this cannot be directly used on
# prod.
# 
# This script must be run both on the orb and the host/laptop. the network
# interface provides a point to point layer 3 tunnel over which internet
# traffic can be routed.
#
# The orb will have ip address 10.0.0.1, and the laptop will have ip 10.0.0.2.
#
# All traffic on 10.0.0.X will go over this tunnel.
# 
# To actually drive network trafic over the tunnel, you can then run
# ```bash
# ssh -w 0:0 worldcoin@<orb-ip>
# ```
#
# If you see "Tunnel forwarding failed" in ssh, it means that probably you
# forgot to run this script on both machines.

set -Eeuxo pipefail

# We assume that if the `orb-id` binary is on the path, this is an orb.
if command -v orb-id; then
    # orb will have this IP
    IP="10.0.0.1"
else
    # laptop will have this IP
    IP="10.0.0.2"
fi

sudo ip tuntap add dev tun0 mode tun # set up a tun network interface
sudo ip addr add ${IP}/24 dev tun0 # assign it an ip and subnet
sudo ip link set dev tun0 up # enable the interface

