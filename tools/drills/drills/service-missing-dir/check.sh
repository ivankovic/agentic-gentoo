#!/bin/bash
# Serving the moved page, not an empty directory made to satisfy the unit.
set -e
systemctl is-active -q "$SVC_NAME.service"
serves "$SVC_PORT" "$SVC_NAME"
