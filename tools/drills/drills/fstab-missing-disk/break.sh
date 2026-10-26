#!/bin/bash
mkdir -p "$MP"
echo "$SPEC $MP $FS defaults 0 2" >> /etc/fstab
systemctl daemon-reload
