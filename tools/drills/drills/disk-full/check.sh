#!/bin/bash
set -e
(( $(df --output=avail -B1M / | tail -1) > 3000 ))
[[ $(stat -c %s "/home/$U_NAME/$MINE") == 1073741824 ]]
installed net-misc/openssh
