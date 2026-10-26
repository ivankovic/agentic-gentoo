#!/bin/bash
useradd -m -G wheel "$U_NAME"
mkdir -p /etc/sudoers.d
echo "$U_NAME ALL=(ALL) NOPASSWD: ALL" > "/etc/sudoers.d/$U_NAME"
# A half-finished edit, left behind.
printf '%s\n' "$BAD" > "/etc/sudoers.d/$FILE"
chmod 440 /etc/sudoers.d/*
! visudo -c >/dev/null 2>&1
