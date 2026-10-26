#!/bin/bash
set -e
[[ $(stat -c %a /tmp) == 1777 && $(stat -c %a /var/tmp) == 1777 ]]
runuser -u "$U_NAME" -- sh -c "f=\$(mktemp -p $DIR) && rm \"\$f\""
