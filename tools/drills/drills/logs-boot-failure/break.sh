#!/bin/bash
cat > "/usr/local/bin/$D_NAME" <<X
#!/bin/bash
conf=/etc/$D_NAME/$D_NAME.conf
[[ -r \$conf ]] || { echo "$D_NAME: cannot open \$conf: No such file or directory" >&2; exit 2; }
port=\$(sed -n 's/^port *= *//p' \$conf)
(( port > 0 && port < 65536 )) || { echo "$D_NAME: \$conf: port \$port out of range" >&2; exit 2; }
exec sleep infinity
X
chmod 755 "/usr/local/bin/$D_NAME"
mkdir -p "/etc/$D_NAME"
[[ $KIND == bad-port ]] && echo "port = 99999" > "/etc/$D_NAME/$D_NAME.conf"
cat > "/etc/systemd/system/$D_NAME.service" <<X
[Unit]
Description=$D_DESC

[Service]
ExecStart=/usr/local/bin/$D_NAME

[Install]
WantedBy=multi-user.target
X
systemctl daemon-reload; systemctl enable "$D_NAME.service" 2>/dev/null
find "/etc/$D_NAME" "/etc/systemd/system/$D_NAME.service" "/usr/local/bin/$D_NAME" -type f | sort > /var/lib/.drill-list
