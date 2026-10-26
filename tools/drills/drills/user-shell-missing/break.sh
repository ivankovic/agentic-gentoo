#!/bin/bash
useradd -m -s /bin/zsh alice
echo 'alice:Wonder-land-42' | chpasswd
grep '^alice:' /etc/shadow | cut -d: -f2 > /var/lib/.drill-alice-hash
