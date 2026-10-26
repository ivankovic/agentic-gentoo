#!/bin/bash
useradd -m "$U_NAME"
echo "$U_NAME:Correct-Horse-7" | chpasswd
grep "^$U_NAME:" /etc/shadow | cut -d: -f2 > /var/lib/.drill-hash
case $KIND in
	locked) passwd -l "$U_NAME" >/dev/null ;;
	expired) usermod -e 2025-01-01 "$U_NAME" ;;
esac
