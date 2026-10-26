#!/bin/bash
set -e
systemctl is-enabled -q "$JOB.timer"
systemctl is-active -q "$JOB.timer"
systemctl show -P NextElapseUSecRealtime "$JOB.timer" | grep -q " $HH:$MM:00"
# And the job itself works.
systemctl start "$JOB.service"
test -s "/var/backups/$JOB.tar.gz"
