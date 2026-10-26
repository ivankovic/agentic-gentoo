#!/bin/bash
set -e
sh=$(getent passwd "$U_NAME" | cut -d: -f7)
grep -qx "$sh" /etc/shells
[[ -x $sh ]]
runuser -u "$U_NAME" -- "$sh" -c 'exit 0'
[[ $(grep "^$U_NAME:" /etc/shadow | cut -d: -f2) == "$(cat /var/lib/.drill-hash)" ]]
