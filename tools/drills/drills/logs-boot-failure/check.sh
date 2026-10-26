#!/bin/bash
set -e
answer_has "$D_NAME"
case $KIND in
	missing-config) answer_has "$D_NAME\.conf|config" 'missing|no such|does not exist|doesn.t exist|not found|absent' ;;
	bad-port) answer_has 'port' '99999|out of range|invalid|too (high|large)' ;;
esac
# Nothing changed.
[[ $(find "/etc/$D_NAME" "/etc/systemd/system/$D_NAME.service" "/usr/local/bin/$D_NAME" -type f | sort) == "$(cat /var/lib/.drill-list)" ]]
systemctl is-failed -q "$D_NAME.service"
