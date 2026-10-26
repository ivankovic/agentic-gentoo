#!/bin/bash
# A hand edit of /etc/hosts broke one entry; the others must stay.
printf '192.168.7.20\tnas.home nas\n192.168.7.21\tprinter.home printer\n192.168.7.22\tpi.home pi\n192.168.7.23\tmedia.home media\n' >> /etc/hosts
grep -P "\t$HOST.home $HOST\$" /etc/hosts | cut -f1 > /var/lib/.drill-ip
case $KIND in
	typo) sed -i "s/\t$HOST.home $HOST\$/\t${HOST%?}.home ${HOST%?}/" /etc/hosts ;;
	commented) sed -i "/\t$HOST.home $HOST\$/s/^/#/" /etc/hosts ;;
	swapped) sed -i "s/^\([0-9.]*\)\t\($HOST.home $HOST\)\$/\2\t\1/" /etc/hosts ;;
esac
! getent hosts "$HOST" >/dev/null
