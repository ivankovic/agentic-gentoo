#!/bin/bash
# Running with the user's settings, wherever the file ended up.
set -e
systemctl is-active -q "$D_NAME.service"
pid=$(systemctl show -P MainPID "$D_NAME.service")
tr '\0' '\n' < "/proc/$pid/environ" | grep -qx "QUEUE_DIR=/var/lib/$D_NAME"
tr '\0' '\n' < "/proc/$pid/environ" | grep -qx "BATCH=50"
