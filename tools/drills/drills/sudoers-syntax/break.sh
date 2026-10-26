#!/bin/bash
useradd -m -G wheel alice
mkdir -p /etc/sudoers.d
echo 'alice ALL=(ALL) NOPASSWD: ALL' > /etc/sudoers.d/alice
# A half-finished edit, left behind.
printf 'Defaults env_keep += "HTTP_PROXY\n' > /etc/sudoers.d/proxy
chmod 440 /etc/sudoers.d/*
