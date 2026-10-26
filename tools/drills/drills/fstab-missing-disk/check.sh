#!/bin/bash
# The missing disk no longer stops the boot (removed, or nofail/noauto), the root entry is still
# there, and the machine came back up (the runner rebooted it and reached SSH) with nothing failed.
set -e
grep -q '^LABEL=drill / ext4' /etc/fstab
if grep -E '^[^#]*6b1c6a1e-3f0a-4c55-9d7e-1f2a3b4c5d6e' /etc/fstab | grep -qvE 'nofail|noauto'; then exit 1; fi
! systemctl --failed --no-legend | grep -q . || exit 1
[[ $(systemctl is-system-running) == running ]]
