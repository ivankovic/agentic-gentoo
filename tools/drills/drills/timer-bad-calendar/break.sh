#!/bin/bash
cat > /etc/systemd/system/backup.service <<'X'
[Unit]
Description=Nightly backup

[Service]
Type=oneshot
ExecStart=/usr/bin/tar -czf /var/backups/etc.tar.gz /etc
X
cat > /etc/systemd/system/backup.timer <<'X'
[Unit]
Description=Nightly backup

[Timer]
OnCalendar=*-*-* 2:30:00 daily
Persistent=true

[Install]
WantedBy=timers.target
X
mkdir -p /var/backups
systemctl daemon-reload; systemctl enable --now backup.timer 2>/dev/null || true
