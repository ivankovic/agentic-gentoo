#!/bin/bash
mkdir -p /data
echo 'UUID=6b1c6a1e-3f0a-4c55-9d7e-1f2a3b4c5d6e /data ext4 defaults 0 2' >> /etc/fstab
systemctl daemon-reload
