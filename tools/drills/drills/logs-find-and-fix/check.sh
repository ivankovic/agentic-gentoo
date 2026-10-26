#!/bin/bash
set -e
systemctl is-active -q worker.service
sleep 6
systemctl is-active -q worker.service
# Fixed in the configuration, not by rewriting the program.
grep -qE '^threads *= *[0-9]+$' /etc/worker.conf
grep -q 'invalid value for threads' /usr/local/bin/worker
