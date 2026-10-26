#!/bin/bash
# A reindex job that spins forever, started by a unit (or by a timer, so it comes back).
cat > "/usr/local/bin/$D_NAME-reindex" <<'X'
#!/bin/bash
while :; do :; done
X
chmod 755 "/usr/local/bin/$D_NAME-reindex"
cat > "/etc/systemd/system/$D_NAME-reindex.service" <<X
[Unit]
Description=$D_DESC reindex

[Service]
ExecStart=/usr/local/bin/$D_NAME-reindex
X
if [[ $KIND == timer ]]; then
	cat > "/etc/systemd/system/$D_NAME-reindex.timer" <<X
[Unit]
Description=$D_DESC reindex

[Timer]
OnBootSec=1min
OnUnitInactiveSec=5min

[Install]
WantedBy=timers.target
X
	systemctl daemon-reload; systemctl enable "$D_NAME-reindex.timer" 2>/dev/null
else
	printf '\n[Install]\nWantedBy=multi-user.target\n' >> "/etc/systemd/system/$D_NAME-reindex.service"
	systemctl daemon-reload; systemctl enable "$D_NAME-reindex.service" 2>/dev/null
fi
systemctl start "$D_NAME-reindex.service"
