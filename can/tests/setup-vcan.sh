#!/bin/bash

modprobe vcan
ip link add dev vcan0 type vcan
ip link set vcan0 mtu 72
ip link set up vcan0

ip link add dev vcan1 type vcan
ip link set vcan1 mtu 16
ip link set up vcan1

ip link add dev vcan2 type vcan
ip link set vcan2 mtu 16
ip link set up vcan2

ip link add dev vcan3 type vcan
ip link set vcan3 mtu 72
ip link set up vcan3
