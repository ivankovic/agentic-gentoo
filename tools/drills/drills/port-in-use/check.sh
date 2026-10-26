#!/bin/bash
set -e
systemctl is-active -q "$SVC_NAME.service"
serves "$SVC_PORT" "$SVC_NAME"
[[ $(ss -Htlnp "sport = :$SVC_PORT" | grep -c python) == 1 ]]
