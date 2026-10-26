#!/bin/bash
sed -i "s/^$KEY = $BAD\$/$KEY = 4/" "/etc/$D_NAME.conf" && systemctl restart "$D_NAME.service"
