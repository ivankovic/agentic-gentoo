#!/bin/bash
sed -i '/^UUID=6b1c6a1e-3f0a-4c55-9d7e-1f2a3b4c5d6e /d' /etc/fstab && systemctl daemon-reload
