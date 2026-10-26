#!/bin/bash
useradd -m -s "$SH" "$U_NAME"
echo "$U_NAME:Wonder-land-42" | chpasswd
grep "^$U_NAME:" /etc/shadow | cut -d: -f2 > /var/lib/.drill-hash
