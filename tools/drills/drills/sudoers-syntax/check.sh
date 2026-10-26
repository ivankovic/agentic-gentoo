#!/bin/bash
set -e
visudo -c >/dev/null
runuser -u "$U_NAME" -- sudo -n true
# Nobody got a broader rule than before.
! grep -rhsE '^[[:space:]]*(%wheel|ALL[[:space:]])' /etc/sudoers /etc/sudoers.d/ || exit 1
