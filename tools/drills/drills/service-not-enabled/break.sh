#!/bin/bash
web_root "/srv/$SVC_NAME" "$SVC_NAME"
web_unit "/etc/systemd/system/$SVC_NAME.service" "$SVC_DESC" "/srv/$SVC_NAME" "$SVC_PORT"
systemctl daemon-reload
