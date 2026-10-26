#!/bin/bash
set -e
sh=$(getent passwd alice | cut -d: -f7)
grep -qx "$sh" /etc/shells
[[ -x $sh ]]
runuser -u alice -- "$sh" -c 'exit 0'
[[ $(grep '^alice:' /etc/shadow | cut -d: -f2) == "$(cat /var/lib/.drill-alice-hash)" ]]
