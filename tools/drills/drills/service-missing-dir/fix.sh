#!/bin/bash
sed -i "s|^WorkingDirectory=.*|WorkingDirectory=$BASE/$SVC_NAME|" "/etc/systemd/system/$SVC_NAME.service"
systemctl daemon-reload && systemctl restart "$SVC_NAME.service"
