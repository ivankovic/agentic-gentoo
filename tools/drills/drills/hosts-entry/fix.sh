#!/bin/bash
sed -i "/$HOST/d" /etc/hosts
printf '%s\t%s.home %s\n' "$(cat /var/lib/.drill-ip)" "$HOST" "$HOST" >> /etc/hosts
