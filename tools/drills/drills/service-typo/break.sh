#!/bin/bash
web_root "/srv/$SVC_NAME" "$SVC_NAME"
web_unit "/etc/systemd/system/$SVC_NAME.service" "$SVC_DESC" "/srv/$SVC_NAME" "$SVC_PORT" "$BAD" "$RESTART"
systemctl daemon-reload; systemctl enable "$SVC_NAME.service" 2>/dev/null
systemctl start "$SVC_NAME.service" || true
