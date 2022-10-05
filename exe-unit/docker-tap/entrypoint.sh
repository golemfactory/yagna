#!/bin/bash

syslogd

truncate -s 500M /golem/output/dummy.dat

echo "setup env"

echo "1" > /proc/sys/net/ipv4/conf/all/arp_accept
echo "1" > /proc/sys/net/ipv4/conf/all/arp_notify
echo "0" > /proc/sys/net/ipv4/conf/all/rp_filter
echo "1" > /proc/sys/net/ipv4/conf/all/log_martians

echo "setup tap iface"

ip tuntap add mode tap ${TAP_NAME}
ip addr add ${IP_ADDR}/24 brd + dev ${TAP_NAME}
ip link set ${TAP_NAME} mtu 1220

ip link set dev ${TAP_NAME} up

ip route replace default via ${IP_GW}
ip route del 172.17.0.0/16 dev eth0
ip link del eth0

echo "spawn pump"
./pump ${TAP_NAME} "/golem/output/${TAP_NAME}-write" "/golem/output/${TAP_NAME}" ${IP_ADDR} ${IP_GW} 1500 | tee /tmp/pump.log 2>&1 &

echo "spwan sshd"
/usr/sbin/sshd -D
