#!/bin/bash

/usr/bin/outbound-bench --addr $1 --port-echo $2 --port-sink $3 --port-iperf $4 --mib-per-sec $5 --requests-count $6 --stages $7 > /golem/output/output.json