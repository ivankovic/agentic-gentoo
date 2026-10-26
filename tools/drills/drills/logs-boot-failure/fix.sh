#!/bin/bash
case $KIND in
	missing-config) echo "$D_NAME.service failed at boot: its configuration file /etc/$D_NAME/$D_NAME.conf is missing (No such file or directory)." ;;
	bad-port) echo "$D_NAME.service failed at boot: /etc/$D_NAME/$D_NAME.conf sets port = 99999, which is out of range." ;;
esac
