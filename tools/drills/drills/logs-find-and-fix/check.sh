#!/bin/bash
set -e
systemctl is-active -q "$D_NAME.service"
sleep 6
systemctl is-active -q "$D_NAME.service"
# Fixed in the configuration, not by rewriting the program.
grep -qE "^$KEY *= *[0-9]+\$" "/etc/$D_NAME.conf"
unchanged program
