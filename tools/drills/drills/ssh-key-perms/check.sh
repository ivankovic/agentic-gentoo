#!/bin/bash
set -e
sshd -T 2>/dev/null | grep -qix 'strictmodes yes'
ssh -q -i /var/lib/.drill-key -o BatchMode=yes -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
	"$U_NAME@127.0.0.1" true
# Not opened up to everyone either.
! find "/home/$U_NAME" -maxdepth 2 -perm -o+w | grep -q . || exit 1
