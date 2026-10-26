#!/bin/bash
# Someone tested something on the service's port by hand and left it running.
web_root "/srv/$SVC_NAME" "$SVC_NAME"
web_unit "/etc/systemd/system/$SVC_NAME.service" "$SVC_DESC" "/srv/$SVC_NAME" "$SVC_PORT"
mkdir -p /root/test && echo test > /root/test/index.html
(cd /root/test && exec setsid python3 -m http.server "$SVC_PORT") >/dev/null 2>&1 </dev/null &
sleep 2
systemctl daemon-reload; systemctl enable "$SVC_NAME.service" 2>/dev/null
systemctl start "$SVC_NAME.service" 2>/dev/null; sleep 1
! systemctl is-active -q "$SVC_NAME.service"
