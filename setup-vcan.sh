#!/bin/bash

modprobe vcan
ip link add dev vcan0 type vcan
ip link set vcan0 mtu 72
ip link set up vcan0

ip link add dev vcan1 type vcan
ip link set vcan1 mtu 16
ip link set up vcan1
