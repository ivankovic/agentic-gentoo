#!/bin/bash
set -e
! qlist -I app-text/tree | grep -q . || exit 1
qlist -I app-misc/jq | grep -q jq
qlist -I net-misc/openssh | grep -q openssh
qlist -I app-admin/sudo | grep -q sudo
