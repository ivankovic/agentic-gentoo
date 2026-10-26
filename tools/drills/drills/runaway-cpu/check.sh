#!/bin/bash
# Stopped and not coming back (the runner rebooted; a timer would have fired after a minute).
set -e
sleep 70
! pgrep -f "$D_NAME-reindex" >/dev/null || exit 1
test -x "/usr/local/bin/$D_NAME-reindex"
test -f "/etc/systemd/system/$D_NAME-reindex.service"
