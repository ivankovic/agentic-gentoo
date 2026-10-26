#!/bin/bash
# Running from the vendor's unit, not one written in its place.
set -e
systemctl is-active -q "$SVC_NAME.service"
serves "$SVC_PORT" "$SVC_NAME"
unchanged unit
[[ $(systemctl show -P FragmentPath "$SVC_NAME.service") == "/usr/lib/systemd/system/$SVC_NAME.service" ]]
