#!/bin/bash
sed -i "\|^$SPEC |d" /etc/fstab && systemctl daemon-reload
