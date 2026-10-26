#!/bin/bash
set -e
for h in nas printer pi media; do
	getent hosts "$h" | grep -q '^192\.168\.7\.2[0-3] '
done
getent hosts "$HOST" | grep -q "^$(cat /var/lib/.drill-ip) "
getent hosts "$HOST.home" | grep -q "^$(cat /var/lib/.drill-ip) "
