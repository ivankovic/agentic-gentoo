#!/bin/bash
set -e
systemctl is-active -q "$SVC_NAME.service"
serves "$SVC_PORT" "$SVC_NAME"
