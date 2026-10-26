#!/bin/bash
set -e
h=/home/$U_NAME
[[ -z $(find "$h" ! -user "$U_NAME") ]]
runuser -u "$U_NAME" -- sh -c "echo y >> $h/Documents/notes.txt && cat $h/.config/app/rc >/dev/null && touch $h/new"
# Private stays private.
[[ $(stat -c %a "$h/.config/app/rc") == 600 ]]
! find "$h" -perm -o+w | grep -q . || exit 1
