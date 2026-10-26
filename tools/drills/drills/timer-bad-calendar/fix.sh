#!/bin/bash
sed -i 's|^OnCalendar=.*|OnCalendar=*-*-* 02:30:00|' /etc/systemd/system/backup.timer
systemctl daemon-reload && systemctl restart backup.timer
