#!/bin/bash
cat > "/etc/systemd/system/$JOB.service" <<X
[Unit]
Description=Nightly $JOB

[Service]
Type=oneshot
ExecStart=/usr/bin/tar -czf /var/backups/$JOB.tar.gz /etc
X
case $KIND in
	daily-suffix) cal="*-*-* ${HH#0}:$MM:00 daily" ;;
	dot) cal="*-*-* $HH.$MM" ;;
	not-enabled) cal="*-*-* $HH:$MM:00" ;;
esac
cat > "/etc/systemd/system/$JOB.timer" <<X
[Unit]
Description=Nightly $JOB

[Timer]
OnCalendar=$cal
Persistent=true

[Install]
WantedBy=timers.target
X
mkdir -p /var/backups
systemctl daemon-reload
if [[ $KIND == not-enabled ]]; then
	# The service was enabled instead of the timer (and it has no [Install] section).
	systemctl enable "$JOB.service" 2>/dev/null || true
else
	systemctl enable --now "$JOB.timer" 2>/dev/null || true
fi
