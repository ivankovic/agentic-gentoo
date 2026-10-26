#!/bin/bash
# The page moved; the unit still points at the old place.
web_root "$BASE/$SVC_NAME" "$SVC_NAME"
web_unit "/etc/systemd/system/$SVC_NAME.service" "$SVC_DESC" "/srv/$SVC_NAME" "$SVC_PORT"
systemctl daemon-reload; systemctl enable "$SVC_NAME.service" 2>/dev/null
systemctl start "$SVC_NAME.service" || true
