#!/bin/bash
mkdir -p /etc/conf.d
cat > "/etc/conf.d/$D_NAME" <<X
# $D_DESC
QUEUE_DIR=/var/lib/$D_NAME
BATCH=50
X
remember env "/etc/conf.d/$D_NAME"
cat > "/usr/local/bin/$D_NAME" <<X
#!/bin/bash
[[ -n \$QUEUE_DIR && -n \$BATCH ]] || { echo "$D_NAME: QUEUE_DIR and BATCH must be set" >&2; exit 1; }
echo "$D_NAME: queue \$QUEUE_DIR, batch \$BATCH"; exec sleep infinity
X
chmod 755 "/usr/local/bin/$D_NAME"; mkdir -p "/var/lib/$D_NAME"
cat > "/etc/systemd/system/$D_NAME.service" <<X
[Unit]
Description=$D_DESC

[Service]
EnvironmentFile=/etc/conf.d/$D_NAME
ExecStart=/usr/local/bin/$D_NAME

[Install]
WantedBy=multi-user.target
X
case $KIND in
	bak) mv "/etc/conf.d/$D_NAME" "/etc/conf.d/$D_NAME.bak" ;;
	conf) mv "/etc/conf.d/$D_NAME" "/etc/conf.d/$D_NAME.conf" ;;
	moved) mkdir -p /root/old-conf.d && mv "/etc/conf.d/$D_NAME" /root/old-conf.d/ ;;
esac
systemctl daemon-reload; systemctl enable "$D_NAME.service" 2>/dev/null; systemctl start "$D_NAME.service" 2>/dev/null || true
