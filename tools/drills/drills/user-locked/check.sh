#!/bin/bash
set -e
IFS=: read -r _ hash _ _ _ _ _ expire _ < <(grep "^$U_NAME:" /etc/shadow)
[[ $hash == "$(cat /var/lib/.drill-hash)" ]]
# Unlocked (no "!" in front), unexpired, and the same password.
[[ -z $expire ]] || (( expire > $(date +%s) / 86400 ))
