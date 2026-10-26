#!/bin/bash
sed -i "s|^ExecStart=$BAD|ExecStart=/usr/bin/python3|" "/etc/systemd/system/$SVC_NAME.service"
# With Restart=, it restarted until systemd gave up; the counter has to be cleared too.
systemctl reset-failed "$SVC_NAME.service"
systemctl daemon-reload && systemctl restart "$SVC_NAME.service"
