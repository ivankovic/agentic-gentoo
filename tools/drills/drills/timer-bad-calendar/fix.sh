#!/bin/bash
sed -i "s|^OnCalendar=.*|OnCalendar=*-*-* $HH:$MM:00|" "/etc/systemd/system/$JOB.timer"
systemctl daemon-reload && systemctl enable "$JOB.timer" && systemctl restart "$JOB.timer"
