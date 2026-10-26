#!/bin/bash
set -e
visudo -c >/dev/null
runuser -u alice -- sudo -n true
# alice did not get a broader rule than before.
! grep -rhsE '^[[:space:]]*%wheel' /etc/sudoers /etc/sudoers.d/ || exit 1
