#!/bin/bash
set -e
systemctl is-enabled -q backup.timer
systemctl is-active -q backup.timer
# The next run is at 02:30.
systemctl show -P NextElapseUSecRealtime backup.timer | grep -q ' 02:30:00'
# And the job itself works.
systemctl start backup.service
test -s /var/backups/etc.tar.gz
