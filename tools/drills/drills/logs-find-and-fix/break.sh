#!/bin/bash
cat > "/etc/$D_NAME.conf" <<X
# $D_NAME settings
queue = /var/lib/$D_NAME
$KEY = $BAD
X
mkdir -p "/var/lib/$D_NAME"
cat > "/usr/local/bin/$D_NAME" <<X
#!/bin/bash
v=\$(sed -n 's/^$KEY *= *//p' /etc/$D_NAME.conf)
[[ \$v =~ ^[0-9]+\$ ]] || { echo "$D_NAME: /etc/$D_NAME.conf: invalid value for $KEY: '\$v' (expected a number)" >&2; exit 1; }
echo "$D_NAME: running with $KEY=\$v"; exec sleep infinity
X
chmod 755 "/usr/local/bin/$D_NAME"
remember program "/usr/local/bin/$D_NAME"
cat > "/etc/systemd/system/$D_NAME.service" <<X
[Unit]
Description=$D_DESC

[Service]
ExecStart=/usr/local/bin/$D_NAME
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
X
systemctl daemon-reload; systemctl enable --now "$D_NAME.service" 2>/dev/null || true
