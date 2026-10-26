#!/bin/bash
set -e
(( $(df --output=avail -B1M / | tail -1) > 3000 ))
test -s /root/archive/photos-2025.tar
qlist -I net-misc/openssh | grep -q openssh
