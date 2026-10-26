#!/bin/bash
# The unit is the vendor's, in /usr/lib; someone masked it.
web_root "/srv/$SVC_NAME" "$SVC_NAME"
web_unit "/usr/lib/systemd/system/$SVC_NAME.service" "$SVC_DESC" "/srv/$SVC_NAME" "$SVC_PORT"
remember unit "/usr/lib/systemd/system/$SVC_NAME.service"
systemctl daemon-reload; systemctl enable "$SVC_NAME.service" 2>/dev/null; systemctl mask "$SVC_NAME.service" 2>/dev/null
