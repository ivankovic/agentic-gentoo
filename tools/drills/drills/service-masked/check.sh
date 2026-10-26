#!/bin/bash
set -e
systemctl is-active -q hello.service
for i in $(seq 10); do curl -sf http://127.0.0.1:8080/ >/dev/null && exit 0; sleep 1; done
exit 1
