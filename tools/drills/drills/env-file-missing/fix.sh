#!/bin/bash
case $KIND in
	bak) mv "/etc/conf.d/$D_NAME.bak" "/etc/conf.d/$D_NAME" ;;
	conf) mv "/etc/conf.d/$D_NAME.conf" "/etc/conf.d/$D_NAME" ;;
	moved) mv "/root/old-conf.d/$D_NAME" /etc/conf.d/ ;;
esac
systemctl restart "$D_NAME.service"
