#!/bin/bash
set -e
runuser -u "$U_NAME" -- sh -c "cat /srv/$GRP/plan.txt >/dev/null && touch /srv/$GRP/.drill-$U_NAME"
# The directory is still the team's only.
[[ $(stat -c '%a %U %G' "/srv/$GRP") == "2770 root $GRP" ]]
id -nG "$U_NAME" | grep -qw "$GRP"
